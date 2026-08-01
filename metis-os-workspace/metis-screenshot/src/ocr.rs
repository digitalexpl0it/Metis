/// Recognise all text in an editor image. The caller owns presentation and
/// clipboard actions so extracted text remains selectable instead of being
/// copied immediately with no way to inspect it.
pub fn run_image(image: &image::RgbaImage) -> Result<String, String> {
    if std::process::Command::new("tesseract")
        .arg("--version")
        .output()
        .is_err()
    {
        return Err(
            "Tesseract OCR is unavailable. Reinstall or upgrade the Metis desktop package.".into(),
        );
    }
    let temp = std::env::temp_dir().join(format!("metis-ocr-{}.png", std::process::id()));
    if let Err(error) = image.save(&temp) {
        tracing::warn!(%error, "unable to write OCR crop");
        return Err(format!("Could not write OCR crop: {error}"));
    }
    let result = match std::process::Command::new("tesseract")
        .arg(&temp)
        .args(["stdout", "-l", "eng", "--psm", "3"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !text.is_empty() {
                Ok(text)
            } else {
                Err("No text was found in this image".into())
            }
        }
        Ok(output) => Err(format!(
            "Tesseract failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Err(format!("Unable to start tesseract: {error}")),
    };
    if let Err(error) = std::fs::remove_file(&temp) {
        tracing::debug!(%error, path = %temp.display(), "unable to remove temporary OCR image");
    }
    result
}
