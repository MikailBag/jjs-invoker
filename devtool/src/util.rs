use std::{fs, io::ErrorKind::*};

pub fn get_project_dir() -> String {
    let mut path = std::env::current_dir().unwrap();
    loop {
        let mut manifest_path = path.clone();
        manifest_path.push("Cargo.toml");
        match std::fs::read(manifest_path) {
            Ok(content) => {
                let content = String::from_utf8_lossy(&content);
                if content.contains("workspace") {
                    return path.to_str().unwrap().to_string();
                }
            }
            Err(_e) => (),
        }
        path = path
            .parent()
            .expect("JJS dir not found. Have you launched devtool inside source tree?")
            .into()
    }
}

pub fn ensure_exists(path: &str) -> Result<(), std::io::Error> {
    match fs::create_dir_all(path) {
        Ok(_) => (),
        Err(e) => match e.kind() {
            AlreadyExists => (),
            _ => return Err(e),
        },
    };

    Ok(())
}
