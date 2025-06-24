use std::path::PathBuf;

pub fn friendly_size(size: usize) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = size as f64;
    let mut unit = 0;

    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", size as usize, UNITS[unit])
    } else {
        format!("{:.2} {}", size, UNITS[unit])
    }
}

pub struct TempDir {
    pub dir: PathBuf,
}
impl TempDir {
    pub fn named_in_tmp(name: &str) -> Result<TempDir, std::io::Error> {
        let dir = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&dir)?;
        Ok(TempDir { dir })
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
