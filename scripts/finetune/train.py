#!/usr/bin/env python3
"""
ThaiRAG Local Fine-Tuning Script

Uses Unsloth + LoRA to fine-tune a base model on Alpaca-format JSONL data,
then exports the result as a GGUF file for use with Ollama.

Communicates progress via JSON lines on stdout.

Usage:
    python train.py --base-model llama3.2:3b --data-path /tmp/dataset.jsonl \
      --output-dir /data/finetune/job-xxx --epochs 3 --lr 2e-4 \
      --lora-rank 16 --batch-size 2 --quantization q4_k_m \
      --model-source ollama
"""

import argparse
import json
import os
import signal
import sys
import time


def emit(msg: dict):
    """Print a JSON progress line to stdout."""
    print(json.dumps(msg), flush=True)


def emit_status(message: str):
    emit({"type": "status", "message": message})


def emit_error(message: str):
    emit({"type": "error", "message": message})


def handle_sigterm(signum, frame):
    emit({"type": "cancelled"})
    sys.exit(1)


signal.signal(signal.SIGTERM, handle_sigterm)


def resolve_ollama_model(model_name: str) -> str:
    """Resolve an Ollama model name to its local file path."""
    # Ollama stores models under ~/.ollama/models
    ollama_dir = os.path.expanduser("~/.ollama/models")
    # Try common patterns
    manifest_dir = os.path.join(ollama_dir, "manifests", "registry.ollama.ai", "library")

    # For models like "llama3.2:3b", split into name:tag
    parts = model_name.split(":")
    name = parts[0]
    tag = parts[1] if len(parts) > 1 else "latest"

    manifest_path = os.path.join(manifest_dir, name, tag)
    if os.path.exists(manifest_path):
        # Read manifest to find the model blob
        with open(manifest_path) as f:
            manifest = json.load(f)
        for layer in manifest.get("layers", []):
            if layer.get("mediaType") == "application/vnd.ollama.image.model":
                digest = layer["digest"].replace(":", "-")
                blob_path = os.path.join(ollama_dir, "models", "blobs", digest)
                if os.path.exists(blob_path):
                    return blob_path
    # Fallback: treat as HuggingFace model name
    return model_name


def main():
    parser = argparse.ArgumentParser(description="ThaiRAG Fine-tuning Script")
    parser.add_argument("--base-model", required=True, help="Base model name or HF repo")
    parser.add_argument("--data-path", required=True, help="Path to Alpaca JSONL dataset")
    parser.add_argument("--output-dir", required=True, help="Output directory for GGUF")
    parser.add_argument("--epochs", type=int, default=3, help="Number of training epochs")
    parser.add_argument("--lr", type=float, default=2e-4, help="Learning rate")
    parser.add_argument("--lora-rank", type=int, default=16, help="LoRA rank")
    parser.add_argument("--lora-alpha", type=int, default=16, help="LoRA alpha")
    parser.add_argument("--batch-size", type=int, default=2, help="Batch size")
    parser.add_argument("--warmup-ratio", type=float, default=0.03, help="Warmup ratio")
    parser.add_argument("--max-seq-length", type=int, default=2048, help="Max sequence length")
    parser.add_argument("--quantization", default="q4_k_m", help="GGUF quantization method")
    parser.add_argument("--model-source", default="huggingface",
                        choices=["ollama", "huggingface"], help="Where to load model from")
    args = parser.parse_args()

    start_time = time.time()
    os.makedirs(args.output_dir, exist_ok=True)

    # ── Step 1: Resolve model path ──
    emit_status("Resolving base model...")

    model_name = args.base_model
    if args.model_source == "ollama":
        model_name = resolve_ollama_model(args.base_model)
        emit_status(f"Resolved Ollama model to: {model_name}")

    # ── Step 2: Load base model ──
    emit_status(f"Loading base model: {model_name}...")

    try:
        from unsloth import FastLanguageModel
    except ImportError:
        emit_error("unsloth is not installed. Run: pip install unsloth")
        sys.exit(1)

    try:
        model, tokenizer = FastLanguageModel.from_pretrained(
            model_name=model_name,
            max_seq_length=args.max_seq_length,
            dtype=None,  # auto-detect
            load_in_4bit=True,
        )
    except Exception as e:
        emit_error(f"Failed to load model: {e}")
        sys.exit(1)

    emit_status("Applying LoRA adapter...")

    # ── Step 3: Apply LoRA ──
    model = FastLanguageModel.get_peft_model(
        model,
        r=args.lora_rank,
        lora_alpha=args.lora_alpha,
        target_modules=[
            "q_proj", "k_proj", "v_proj", "o_proj",
            "gate_proj", "up_proj", "down_proj",
        ],
        lora_dropout=0,
        bias="none",
        use_gradient_checkpointing="unsloth",
    )

    # ── Step 4: Load dataset ──
    emit_status("Loading training dataset...")

    from datasets import load_dataset

    alpaca_prompt = (
        "Below is an instruction that describes a task, paired with an input that provides "
        "further context. Write a response that appropriately completes the request.\n\n"
        "### Instruction:\n{instruction}\n\n### Input:\n{input}\n\n### Response:\n{output}"
    )

    def format_prompts(examples):
        texts = []
        for instruction, inp, output in zip(
            examples["instruction"], examples["input"], examples["output"]
        ):
            text = alpaca_prompt.format(
                instruction=instruction, input=inp, output=output
            ) + tokenizer.eos_token
            texts.append(text)
        return {"text": texts}

    dataset = load_dataset("json", data_files=args.data_path, split="train")
    dataset = dataset.map(format_prompts, batched=True)

    emit_status(f"Dataset loaded: {len(dataset)} examples")

    # ── Step 5: Train ──
    emit_status("Starting training...")

    from trl import SFTTrainer
    from transformers import TrainingArguments, TrainerCallback

    total_steps = max(1, (len(dataset) * args.epochs) // args.batch_size)

    class ProgressCallback(TrainerCallback):
        """Emit JSON progress lines during training."""
        def on_log(self, cb_args, state, control, logs=None, **kwargs):
            if logs and state.global_step > 0:
                emit({
                    "type": "progress",
                    "epoch": int(state.epoch) if state.epoch else 0,
                    "step": state.global_step,
                    "total_steps": state.max_steps or total_steps,
                    "loss": round(logs.get("loss", 0), 4),
                    "lr": logs.get("learning_rate", 0),
                })

    training_args = TrainingArguments(
        output_dir=os.path.join(args.output_dir, "checkpoints"),
        num_train_epochs=args.epochs,
        per_device_train_batch_size=args.batch_size,
        learning_rate=args.lr,
        warmup_ratio=args.warmup_ratio,
        logging_steps=max(1, total_steps // 20),  # ~20 progress updates
        save_strategy="no",
        optim="adamw_8bit",
        fp16=True,
        report_to="none",
    )

    trainer = SFTTrainer(
        model=model,
        tokenizer=tokenizer,
        train_dataset=dataset,
        dataset_text_field="text",
        max_seq_length=args.max_seq_length,
        args=training_args,
        callbacks=[ProgressCallback()],
    )

    try:
        result = trainer.train()
        final_loss = result.training_loss if hasattr(result, "training_loss") else 0
    except Exception as e:
        emit_error(f"Training failed: {e}")
        sys.exit(1)

    emit_status("Training complete")

    # ── Step 6: Export to GGUF ──
    quant = args.quantization
    emit_status(f"Exporting to GGUF ({quant})...")

    output_path = os.path.join(args.output_dir, f"model-{quant}.gguf")
    try:
        model.save_pretrained_gguf(
            args.output_dir,
            tokenizer,
            quantization_method=quant,
        )
        # Find the generated GGUF file
        for f in os.listdir(args.output_dir):
            if f.endswith(".gguf"):
                output_path = os.path.join(args.output_dir, f)
                break
    except Exception as e:
        emit_error(f"GGUF export failed: {e}")
        sys.exit(1)

    elapsed = time.time() - start_time

    emit({
        "type": "completed",
        "output_path": output_path,
        "final_loss": round(final_loss, 4),
        "total_time_secs": round(elapsed),
    })


if __name__ == "__main__":
    main()
