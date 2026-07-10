//! Public `solum` provider; filesystem migration is the P3 slice.

pub fn manifest_json() -> &'static str {
    include_str!("manifest.json")
}
