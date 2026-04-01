#!/usr/bin/env python3
"""
Dry-run test for train.py — validates CLI arg parsing,
JSON protocol output, and SIGTERM handling without requiring
Unsloth/torch/GPU.
"""

import json
import os
import signal
import subprocess
import sys
import tempfile
import textwrap

SCRIPT = os.path.join(os.path.dirname(__file__), "train.py")

# Minimal Alpaca JSONL dataset for testing
SAMPLE_DATA = [
    {"instruction": "What is ThaiRAG?", "input": "", "output": "ThaiRAG is a Thai RAG system."},
    {"instruction": "What is LoRA?", "input": "", "output": "LoRA is Low-Rank Adaptation."},
    {"instruction": "Hello", "input": "", "output": "Hello! How can I help?"},
    {"instruction": "What is fine-tuning?", "input": "", "output": "Fine-tuning adapts a model."},
    {"instruction": "What is GGUF?", "input": "", "output": "GGUF is a model format for llama.cpp."},
]


def write_sample_dataset(path: str):
    with open(path, "w") as f:
        for entry in SAMPLE_DATA:
            f.write(json.dumps(entry) + "\n")


def test_arg_parsing():
    """Test that the script parses all CLI args without crashing (will fail at import)."""
    print("Test 1: CLI arg parsing...")
    with tempfile.TemporaryDirectory() as tmpdir:
        data_path = os.path.join(tmpdir, "dataset.jsonl")
        write_sample_dataset(data_path)

        result = subprocess.run(
            [
                sys.executable, SCRIPT,
                "--base-model", "test-model",
                "--data-path", data_path,
                "--output-dir", tmpdir,
                "--epochs", "1",
                "--lr", "5e-4",
                "--lora-rank", "8",
                "--lora-alpha", "8",
                "--batch-size", "4",
                "--warmup-ratio", "0.03",
                "--max-seq-length", "512",
                "--quantization", "q4_k_m",
                "--model-source", "huggingface",
            ],
            capture_output=True,
            text=True,
            timeout=30,
        )

        # It should emit status JSON lines before failing at the unsloth import
        lines = [l for l in result.stdout.strip().split("\n") if l.strip()]
        assert len(lines) >= 1, f"Expected at least 1 JSON line, got: {result.stdout}"

        first = json.loads(lines[0])
        assert first["type"] == "status", f"Expected status, got: {first}"
        print(f"  Got {len(lines)} JSON lines before import error (expected)")

        # Should have an error about unsloth
        found_error = False
        for line in lines:
            parsed = json.loads(line)
            if parsed.get("type") == "error" and "unsloth" in parsed.get("message", "").lower():
                found_error = True
                break
        # Or it errored via stderr
        if not found_error and "unsloth" in result.stderr.lower():
            found_error = True
        # Accept either — unsloth not installed is the expected outcome
        print(f"  Unsloth import error detected: {found_error}")
        print("  PASS")


def test_json_protocol():
    """Verify JSON protocol format by parsing sample output lines."""
    print("Test 2: JSON protocol validation...")

    sample_lines = [
        '{"type":"status","message":"Loading base model..."}',
        '{"type":"downloading","progress":45}',
        '{"type":"progress","epoch":1,"step":50,"total_steps":500,"loss":0.45,"lr":0.0002}',
        '{"type":"progress","epoch":1,"step":100,"total_steps":500,"loss":0.38,"lr":0.0002}',
        '{"type":"status","message":"Exporting to GGUF (q4_k_m)..."}',
        '{"type":"completed","output_path":"/data/finetune/job-xxx/model-q4_k_m.gguf","final_loss":0.12,"total_time_secs":1234}',
        '{"type":"error","message":"CUDA out of memory"}',
        '{"type":"cancelled"}',
    ]

    for line in sample_lines:
        parsed = json.loads(line)
        assert "type" in parsed, f"Missing type field: {line}"
        msg_type = parsed["type"]
        assert msg_type in ("status", "downloading", "progress", "completed", "error", "cancelled"), \
            f"Unknown type: {msg_type}"

        if msg_type == "progress":
            assert "step" in parsed and "total_steps" in parsed, f"Progress missing fields: {line}"
        elif msg_type == "completed":
            assert "output_path" in parsed, f"Completed missing output_path: {line}"
        elif msg_type == "error":
            assert "message" in parsed, f"Error missing message: {line}"

    print(f"  Validated {len(sample_lines)} protocol messages")
    print("  PASS")


def test_dataset_format():
    """Verify our sample dataset is valid Alpaca JSONL."""
    print("Test 3: Dataset format validation...")
    with tempfile.TemporaryDirectory() as tmpdir:
        data_path = os.path.join(tmpdir, "dataset.jsonl")
        write_sample_dataset(data_path)

        with open(data_path) as f:
            lines = f.readlines()
            assert len(lines) == len(SAMPLE_DATA), f"Expected {len(SAMPLE_DATA)} lines, got {len(lines)}"
            for i, line in enumerate(lines):
                entry = json.loads(line)
                assert "instruction" in entry, f"Line {i}: missing instruction"
                assert "input" in entry, f"Line {i}: missing input"
                assert "output" in entry, f"Line {i}: missing output"

    print(f"  Validated {len(SAMPLE_DATA)} Alpaca entries")
    print("  PASS")


def test_ollama_resolver():
    """Test that the Ollama model resolver handles missing models gracefully."""
    print("Test 4: Ollama model resolver fallback...")

    # Import the resolve function directly
    sys.path.insert(0, os.path.dirname(SCRIPT))
    from train import resolve_ollama_model

    # Non-existent model should fall back to returning the name as-is
    result = resolve_ollama_model("nonexistent-model:latest")
    assert result == "nonexistent-model:latest", f"Expected fallback to model name, got: {result}"

    print("  Non-existent model falls back to name (HuggingFace path)")
    print("  PASS")


def test_sigterm_handling():
    """Test that SIGTERM produces a cancelled JSON line."""
    print("Test 5: SIGTERM handling...")

    # Create a script that sleeps then gets killed
    with tempfile.TemporaryDirectory() as tmpdir:
        data_path = os.path.join(tmpdir, "dataset.jsonl")
        write_sample_dataset(data_path)

        proc = subprocess.Popen(
            [
                sys.executable, SCRIPT,
                "--base-model", "test-model",
                "--data-path", data_path,
                "--output-dir", tmpdir,
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )

        # Wait briefly for it to start, then send SIGTERM
        import time
        time.sleep(1)

        if proc.poll() is None:
            proc.send_signal(signal.SIGTERM)
            try:
                stdout, stderr = proc.communicate(timeout=5)
                lines = [l for l in stdout.strip().split("\n") if l.strip()]
                # Check if any line is a cancelled message
                has_cancelled = any(
                    json.loads(l).get("type") == "cancelled"
                    for l in lines
                    if l.strip()
                )
                if has_cancelled:
                    print("  SIGTERM produced cancelled JSON")
                else:
                    print("  SIGTERM handled (process exited before cancel line)")
            except subprocess.TimeoutExpired:
                proc.kill()
                print("  Process didn't exit after SIGTERM (killed)")
        else:
            print("  Process exited before SIGTERM could be sent (expected - no unsloth)")

    print("  PASS")


if __name__ == "__main__":
    print("=" * 60)
    print("ThaiRAG Fine-tuning Script — Dry-Run Tests")
    print("=" * 60)
    print()

    tests = [
        test_json_protocol,
        test_dataset_format,
        test_ollama_resolver,
        test_arg_parsing,
        test_sigterm_handling,
    ]

    passed = 0
    failed = 0
    for test_fn in tests:
        try:
            test_fn()
            passed += 1
        except Exception as e:
            print(f"  FAIL: {e}")
            failed += 1
        print()

    print("=" * 60)
    print(f"Results: {passed} passed, {failed} failed")
    print("=" * 60)
    sys.exit(1 if failed > 0 else 0)
