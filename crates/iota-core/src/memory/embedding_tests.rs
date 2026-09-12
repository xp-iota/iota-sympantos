use crate::memory::embedding::{
    EmbeddingEngine, LOCAL_DIM, canonicalize, cosine, from_blob, local_trigram, to_blob,
};

#[test]
fn local_trigram_produces_normalized_vector() {
    let vec = local_trigram("hello world");
    assert_eq!(vec.len(), LOCAL_DIM);
    let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    assert!(
        (norm - 1.0).abs() < 1e-5,
        "vector should be unit length, got {norm}"
    );
}

#[test]
fn local_trigram_empty_input_returns_empty() {
    // local_trigram takes already-canonicalized text; EmbeddingEngine::embed handles raw empty
    let engine = EmbeddingEngine::default();
    let vec = engine.embed("");
    assert!(vec.is_empty());
    let vec2 = engine.embed("   ");
    assert!(vec2.is_empty());
}

#[test]
fn cosine_identical_vectors_equals_one() {
    let vec = local_trigram("rust programming language");
    let sim = cosine(&vec, &vec);
    assert!(
        (sim - 1.0).abs() < 1e-6,
        "self-similarity should be 1.0, got {sim}"
    );
}

#[test]
fn cosine_orthogonal_vectors_equals_zero() {
    let a = vec![1.0f32, 0.0, 0.0];
    let b = vec![0.0f32, 1.0, 0.0];
    let sim = cosine(&a, &b);
    assert!(
        (sim).abs() < 1e-6,
        "orthogonal vectors should have 0 similarity, got {sim}"
    );
}

#[test]
fn cosine_dimension_mismatch_returns_zero() {
    let a = vec![1.0f32; 128];
    let b = vec![1.0f32; 64];
    assert_eq!(cosine(&a, &b), 0.0);
}

#[test]
fn blob_roundtrip_preserves_values() {
    let original = vec![1.5f32, -2.3, 0.0, std::f32::consts::PI, f32::MIN_POSITIVE];
    let blob = to_blob(&original);
    assert_eq!(blob.len(), original.len() * 4);
    let recovered = from_blob(&blob);
    assert_eq!(original, recovered);
}

#[test]
fn canonicalize_lowercases_and_strips_punctuation() {
    let result = canonicalize("Hello, World! How's it going?");
    assert_eq!(result, "hello world hows it going");
}

#[test]
fn canonicalize_collapses_whitespace() {
    let result = canonicalize("  multiple   spaces\t\ttabs\n\nnewlines  ");
    assert_eq!(result, "multiple spaces tabs newlines");
}

#[test]
fn embedding_engine_default_uses_local() {
    let engine = EmbeddingEngine::default();
    assert!(!engine.is_api());
    let vec = engine.embed("test content");
    assert_eq!(vec.len(), LOCAL_DIM);
}

#[test]
fn embedding_engine_from_config_none_uses_local() {
    let engine = EmbeddingEngine::from_config(None);
    assert!(!engine.is_api());
}

#[test]
fn embedding_engine_from_config_with_base_url_is_api() {
    use crate::config::EmbeddingConfig;
    let cfg = EmbeddingConfig {
        base_url: Some("http://localhost:11434".to_string()),
        api_key: None,
        model: Some("nomic-embed-text".to_string()),
    };
    let engine = EmbeddingEngine::from_config(Some(cfg));
    assert!(engine.is_api());
    // embed() calls API; if Ollama is running returns 768-dim, else falls back to 128-dim local
    let vec = engine.embed("fallback test");
    assert!(
        vec.len() == LOCAL_DIM || vec.len() == 768,
        "expected {} or 768 dims, got {}",
        LOCAL_DIM,
        vec.len()
    );
}

#[test]
fn similar_content_has_higher_cosine_than_unrelated() {
    let engine = EmbeddingEngine::default();
    let rust_vec = engine.embed("rust programming systems language");
    let rust2_vec = engine.embed("systems programming in rust language");
    let weather_vec = engine.embed("weather forecast tomorrow rain");

    let sim_related = cosine(&rust_vec, &rust2_vec);
    let sim_unrelated = cosine(&rust_vec, &weather_vec);
    assert!(
        sim_related > sim_unrelated,
        "related content similarity ({sim_related}) should exceed unrelated ({sim_unrelated})"
    );
}
