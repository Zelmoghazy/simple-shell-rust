use std::io::{stdin, stdout, Write};
use std::process::Command;
use std::env;
use std::path::Path;

// if we cannot get to the home directory we fallback to the root directory
fn get_home_directory() -> String {
    #[cfg(unix)]
    {
        std::env::var("HOME")
            .unwrap_or_else(|_| "/".to_string())
    }

    #[cfg(windows)]
    {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOMEDRIVE").and_then(|drive| 
                std::env::var("HOMEPATH").map(|path| drive + &path)
            ))
            .unwrap_or_else(|_| "C:\\".to_string())
    }
}


fn resolve_path(path: &str) -> String 
{
    if path == "~" {
        return get_home_directory();
    }
    
    if path.starts_with("~/") {
        let home = get_home_directory();
        return format!("{}/{}", home.trim_end_matches('/'), &path[2..]);
    }
    
    path.to_string()
}

fn main() 
{
    let hostname = match env::var("HOSTNAME") {
        Ok(host) => host,
        Err(_) => {
            match Command::new("hostname").output() {
                Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
                Err(_) => "unknown".to_string()
            }
        }
    };

    loop {
        print!("{}> ", hostname);
        stdout().flush().unwrap();  // panic on error

        let mut input = String::new();
        stdin().read_line(&mut input).unwrap();
    
        let mut tokens = input.trim().split_whitespace(); 
        let cmd = match tokens.next() {
            Some(c) => c,
            None => continue, // Skip empty input
        };
        let args = tokens;

        /*
            Some commands have to be built into the shell program itself 
            because they cannot work if they are external.

            cd is one such since if it were external, it could only change 
            its own directory; it couldn't affect the current working directory 
            of the shell. 
         */
        match cmd
        {
            "cd" => {
                // Default to the home directory if no argument is provided
                let new_dir = args.peekable().peek().map_or_else(
                    || get_home_directory(),
                    |x| resolve_path(x),
                );

                let root = Path::new(&new_dir);
                if let Err(e) = env::set_current_dir(&root) {
                    eprintln!("Failed to change directory to '{}': {}", new_dir, e);
                }
            },
            "pwd" => {
                match env::current_dir() {
                    Ok(path) => println!("{}", path.display()),
                    Err(e) => eprintln!("Failed to get current directory: {}", e),
                }
            },
            "exit" => {
                std::process::exit(0);
            },
            cmd => {
                let spawn_result = Command::new(cmd)
                    .args(args)
                    .spawn();
        
                match spawn_result {
                    Ok(mut child) => {
                        if let Err(e) = child.wait() {
                            eprintln!("Error waiting for child process: {}", e);
                        }
                    },
                    Err(e) => eprintln!("{}: {}",input.trim_end(), e)
                }
            }
        }
    }
}