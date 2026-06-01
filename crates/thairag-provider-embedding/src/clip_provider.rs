use std::sync::Arc;

use async_trait::async_trait;
use fastembed::{
    EmbeddingModel as FastEmbedTextModel, ImageEmbedding, ImageEmbeddingModel, ImageInitOptions,
    InitOptions, TextEmbedding,
};
use thairag_core::ThaiRagError;
use thairag_core::error::Result;
use tracing::info;

/// CLIP ViT-B-32 produces 512-dim embeddings; the image and text encoders share
/// this space, so a text-query vector is directly comparable to an image vector.
const CLIP_VIT_B32_DIM: usize = 512;

/// Local fastembed CLIP provider. Holds both the image encoder and the matching
/// CLIP text encoder so text→image and image→image retrieval share one vector
/// space. Both models are local ONNX (downloaded on first use); no API cost.
pub struct FastEmbedClipProvider {
    image_model: Arc<ImageEmbedding>,
    text_model: Arc<TextEmbedding>,
    dimension: usize,
}

impl FastEmbedClipProvider {
    pub fn new(model_name: &str) -> Self {
        let (image_variant, text_variant, dimension) = match model_name {
            "clip-vit-b-32" | "Qdrant/clip-ViT-B-32-vision" => (
                ImageEmbeddingModel::ClipVitB32,
                FastEmbedTextModel::ClipVitB32,
                CLIP_VIT_B32_DIM,
            ),
            _ => {
                info!(
                    model_name,
                    "Unknown CLIP model name, falling back to clip-vit-b-32"
                );
                (
                    ImageEmbeddingModel::ClipVitB32,
                    FastEmbedTextModel::ClipVitB32,
                    CLIP_VIT_B32_DIM,
                )
            }
        };

        info!(
            ?image_variant,
            "Initializing fastembed CLIP models (this may download on first run)"
        );
        let image_model = ImageEmbedding::try_new(ImageInitOptions::new(image_variant))
            .expect("Failed to initialize fastembed CLIP image model");
        let text_model = TextEmbedding::try_new(InitOptions::new(text_variant))
            .expect("Failed to initialize fastembed CLIP text model");
        info!("fastembed CLIP models initialized successfully");

        Self {
            image_model: Arc::new(image_model),
            text_model: Arc::new(text_model),
            dimension,
        }
    }
}

#[async_trait]
impl thairag_core::traits::ImageEmbeddingModel for FastEmbedClipProvider {
    async fn embed_images(&self, images: &[Vec<u8>]) -> Result<Vec<Vec<f32>>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        let model = Arc::clone(&self.image_model);
        let images: Vec<Vec<u8>> = images.to_vec();

        tokio::task::spawn_blocking(move || {
            let refs: Vec<&[u8]> = images.iter().map(|v| v.as_slice()).collect();
            model
                .embed_bytes(&refs, None)
                .map_err(|e| ThaiRagError::Embedding(e.to_string()))
        })
        .await
        .map_err(|e| ThaiRagError::Embedding(format!("spawn_blocking join error: {e}")))?
    }

    async fn embed_query_text(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let model = Arc::clone(&self.text_model);
        let texts: Vec<String> = texts.to_vec();

        tokio::task::spawn_blocking(move || {
            model
                .embed(texts, None)
                .map_err(|e| ThaiRagError::Embedding(e.to_string()))
        })
        .await
        .map_err(|e| ThaiRagError::Embedding(format!("spawn_blocking join error: {e}")))?
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use thairag_core::traits::ImageEmbeddingModel as _;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (na * nb)
    }

    fn solid_png(r: u8, g: u8, b: u8) -> Vec<u8> {
        use image::{ImageFormat, RgbImage};
        use std::io::Cursor;
        let img = RgbImage::from_pixel(64, 64, image::Rgb([r, g, b]));
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    // Downloads the CLIP ONNX model (~350MB) on first run, so it is ignored in
    // CI. Run locally with: cargo test -p thairag-provider-embedding -- --ignored
    #[tokio::test]
    #[ignore]
    async fn clip_embeds_and_shares_space() {
        let provider = FastEmbedClipProvider::new("clip-vit-b-32");
        assert_eq!(provider.dimension(), CLIP_VIT_B32_DIM);

        let red = solid_png(220, 20, 20);
        let blue = solid_png(20, 20, 220);
        let img_vecs = provider.embed_images(&[red, blue]).await.unwrap();
        assert_eq!(img_vecs.len(), 2);
        assert_eq!(img_vecs[0].len(), CLIP_VIT_B32_DIM);

        let txt = provider
            .embed_query_text(&["a red image".to_string(), "a blue image".to_string()])
            .await
            .unwrap();
        assert_eq!(txt[0].len(), CLIP_VIT_B32_DIM);

        // Shared-space sanity: "a red image" should sit closer to the red image
        // than to the blue one, proving image+text encoders are comparable.
        let red_to_red = cosine(&txt[0], &img_vecs[0]);
        let red_to_blue = cosine(&txt[0], &img_vecs[1]);
        assert!(
            red_to_red > red_to_blue,
            "expected red-text closer to red-image: {red_to_red} vs {red_to_blue}"
        );

        // Empty inputs must short-circuit to empty output without error.
        assert!(provider.embed_images(&[]).await.unwrap().is_empty());
        assert!(provider.embed_query_text(&[]).await.unwrap().is_empty());
    }
}
