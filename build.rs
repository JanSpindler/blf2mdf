use std::process::Command;

fn main() {
    // List of Python packages to install
    let packages = ["asammdf", "tqdm", "numpy"];

    // Run the pip install command
    let status = Command::new("python")
        .arg("-m")
        .arg("pip")
        .arg("install")
        .args(&packages)
        .status()
        .expect("Failed to execute pip install");

    if !status.success() {
        panic!("Failed to install required Python packages");
    }
}
