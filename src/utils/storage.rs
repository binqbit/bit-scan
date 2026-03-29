use std::{io::Write, path::Path};

pub fn save_private_key_to_file(
    public_key: &str,
    private_key: [u8; 32],
    file_path: &str,
) -> std::io::Result<()> {
    if !Path::new(file_path).exists() {
        std::fs::create_dir_all(file_path)?;
    }

    let priv_hex = super::private_key_to_hex(private_key);
    let mut file = std::fs::File::create(format!("{}/{}.priv", file_path, public_key))?;
    file.write_all(priv_hex.as_bytes())?;
    Ok(())
}
