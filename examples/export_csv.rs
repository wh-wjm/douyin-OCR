use anyhow::{Context, Result};
use std::path::PathBuf;
use whwjm_ocr::export_csv;

fn main() -> Result<()> {
    let image_dir = std::env::args()
        .nth(1)
        .context("usage: cargo run --example export_csv -- <image_dir>")?;
    let image_dir = PathBuf::from(image_dir);

    let summary = export_csv(&image_dir, |progress| {
        println!(
            "processing {}/{} {}{}",
            progress.current,
            progress.total,
            progress.image_path.display(),
            if progress.cache_hit {
                " (cache hit)"
            } else {
                ""
            }
        );
    })?;

    println!("exported {}", summary.live_csv_path.display());
    println!("exported {}", summary.video_csv_path.display());

    Ok(())
}
