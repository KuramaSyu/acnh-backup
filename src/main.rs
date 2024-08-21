use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use anyhow::{Context, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Select};
use std::{borrow::Cow, fs::{self, File}, iter::Zip};
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::env;
use zip::{write::FileOptions, ZipWriter};
use zip::result::ZipError;
use zip_extensions::{write::ZipWriterExtensions, zip_create_from_directory};

fn main() -> Result<()> {
    // Enable raw mode for interactive terminal input
    enable_raw_mode().expect("Failed to enable raw mode");

    // Enter the alternate screen
    execute!(std::io::stdout(), EnterAlternateScreen).expect("Failed to enter alternate screen");

    loop {
        // Check if a key event occurred
        if event::poll(std::time::Duration::from_millis(500))? {
            if let Event::Key(key_event) = event::read()? {
                if key_event.code == KeyCode::Char('q') {
                    // Leave the alternate screen
                    execute!(std::io::stdout(), LeaveAlternateScreen).expect("Failed to leave alternate screen");

                    // Disable raw mode
                    disable_raw_mode().expect("Failed to disable raw mode");

                    println!("Exiting...");
                    process::exit(0);
                }
            }
        }

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("What would you like to do?")
            .item("Backup")
            .item("Restore")
            .item("Exit")
            .interact_opt()
            .expect("Failed to get user selection");

        let selection = match selection {
            Some(selection) => selection,
            None => 2,
        };

        match selection {
            0 => backup_directory(),
            1 => restore_directory(),
            2 => {
                // Leave the alternate screen
                execute!(std::io::stdout(), LeaveAlternateScreen).expect("Failed to leave alternate screen");

                // Disable raw mode
                disable_raw_mode().expect("Failed to disable raw mode");

                println!("Exiting...");
                process::exit(0);
            }
            _ => unreachable!(),
        }
    }
}

fn backup_directory() {
    let source_dir = get_source_dir();
    let target_dir = get_target_dir();

    if !target_dir.exists() {

        fs::create_dir_all(&target_dir).expect("Failed to create target directory");
    }

    let backup_name = format!(
        "0000000000000001_{}.zip",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    );
    let backup_path = target_dir.join(backup_name);

    println!("Backing up directory to: {}", backup_path.display());
    create_zip_backup(&source_dir, &backup_path).expect("Failed to create backup");
    println!("Backup complete.");

    // Prompt the user to continue
    Confirm::with_theme(&ColorfulTheme::default())
        .default(true)
        .with_prompt("Press Enter to continue")
        .interact_opt()
        .expect("Failed to get user input");
}

fn restore_directory() {
    let target_dir = get_source_dir();
    let backup_dir = get_target_dir();

    let mut backup_files: Vec<_> = fs::read_dir(&backup_dir)
        .expect("Failed to read backup directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.path().is_file() && entry.path().extension().unwrap_or_default() == "zip" {
                Some(entry.path().file_name().unwrap().to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();
    backup_files.insert(0, "Go back".to_string());

    if backup_files.is_empty() {
        println!("No backups found in the backup directory.");
        return;
    }

    let selected_backup = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select a backup to restore")
        .items(&backup_files)
        .interact_opt()
        .expect("Failed to get user selection");

    let selected_backup = match selected_backup {
        Some(selected_backup) => selected_backup,
        None => return,
    };

    if &backup_files[selected_backup] == "Go back" {
        return;
    }

    let backup_path = backup_dir.join(&backup_files[selected_backup]);
    println!("Restoring directory from: {}", backup_path.display());
    fs::remove_dir_all(&target_dir).expect("Failed to remove target directory");
    fs::create_dir_all(&target_dir).expect("Failed to create target directory");
    extract_zip_backup(&backup_path, &target_dir).expect("Failed to restore backup");
    println!("Restore complete.");

    // Prompt the user to continue
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Press Enter to continue")
        .interact_opt()
        .expect("Failed to get user input");
}

fn get_source_dir() -> PathBuf {
    let username = whoami::username();
    if cfg!(target_os = "windows") {
        Path::new(&format!(r"C:\Users\{username}\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001")).to_path_buf()
    } else {
        Path::new(&format!(r"/home/{username}/.config/Ryujinx/bis/user/save/0000000000000001")).to_path_buf()
    }
}

fn get_target_dir() -> PathBuf {
    let username = whoami::username();
    if cfg!(target_os = "windows") {
        Path::new(&format!(r"C:\Users\{username}\Desktop\Backups")).to_path_buf()
    } else {
        let username = env::var("USER").expect("Failed to get user name").to_string();
        Path::new(&format!(r"/home/{username}/Backups")).to_path_buf()
    }
}

fn create_zip_backup(source_dir: &Path, backup_path: &Path) -> Result<()>{

    let file = File::create(backup_path)?;
    let zip = ZipWriter::new(file);
    zip.create_from_directory(&source_dir.into())?;
    Ok(())
}

fn extract_zip_backup(backup_path: &Path, target_dir: &Path) -> Result<(), ZipError> {
    let file = std::fs::File::open(backup_path)?;
    let mut zip = zip::ZipArchive::new(file)?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let outpath = if file.name().ends_with('/') {
            target_dir.join(file.name())
        } else {
            target_dir.join(file.name())
        };

        if (*file.name()).ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(&p)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }

        // Get and Set permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                fs::set_permissions(&outpath, fs::Permissions::from_mode(mode))?;
            }
        }
    }

    Ok(())
}