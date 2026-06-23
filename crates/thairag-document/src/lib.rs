pub mod ai;
pub mod chunker;
pub mod conversion_fidelity;
pub mod converter;
pub mod image;
pub mod office_media;
pub mod office_tables;
pub mod pdf_rasterizer;
pub mod pdfium_engine;
pub mod pipeline;
pub mod region_router;
pub mod semantic;
pub mod semantic_prompts;
pub mod smart_pdf;
pub mod table_extractor;
pub mod table_lattice;
pub mod table_stream;
pub mod text_utils;
pub mod thai_chunker;
pub mod tree_builder;
pub mod window_chunker;

pub use image::{
    IMAGE_MIME_TYPES, describe_image, extract_image_metadata, format_placeholder_description,
    is_image_mime,
};
pub use pipeline::{DocumentPipeline, ProcessedDocument, StepCallback};
pub use semantic::{PageStrategy, StrategyThresholds, select_page_strategy};
pub use smart_pdf::ExtractedImageBlob;
pub use table_extractor::{extract_tables, looks_like_table, table_to_markdown};
pub use thai_chunker::{ThaiAwareChunker, is_thai_text, thai_char_ratio};
pub use window_chunker::{build_parent_document_chunks, build_sentence_window_chunks};
