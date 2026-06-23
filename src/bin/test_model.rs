use anyhow::Result;
use whwjm_ocr::OcrClient;

const TEST_IMAGES: [&str; 2] = ["models/test1.jpg", "models/test2.png"];

fn main() -> Result<()> {
    let client = OcrClient::new_default()?;

    println!("model loaded");

    for image_path in TEST_IMAGES {
        let image = image::open(image_path)?;
        let results = client.recognize_image(&image)?;

        println!("\nimage: {image_path}");
        println!("size: {} x {}", image.width(), image.height());
        println!("text regions: {} after merge", results.len());

        for (index, item) in results.iter().enumerate() {
            let special_marker = if item.is_special { "*" } else { "" };
            println!(
                "{:>2}. [{:.2}%] {}{} ({}, {}, {} x {})",
                index + 1,
                item.confidence * 100.0,
                special_marker,
                item.text,
                item.left,
                item.top,
                item.width,
                item.height
            );
        }
    }

    Ok(())
}
