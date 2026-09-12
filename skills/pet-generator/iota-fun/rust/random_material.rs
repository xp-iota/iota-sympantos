pub fn random_material() -> &'static str {
    let materials = ["wood", "metal", "glass", "plastic", "stone"];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    materials[nanos % materials.len()]
}
