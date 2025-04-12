#![deny(clippy::unwrap_used)]

use anyhow::Result;
use include_dir::include_dir;

static STATIC_FILES: include_dir::Dir = include_dir!("static");

pub fn get(filename: &str) -> Result<&'static str> {
    let file = STATIC_FILES
        .get_file(filename)
        .ok_or_else(|| anyhow::anyhow!("File not found"))?;
    file.contents_utf8()
        .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8"))
}
