use std::io::{stdin, stdout, Write};
use std::process::Command;
use std::env;
use std::path::Path;
use std::collections::VecDeque;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, ClearType},
    style::{Color, SetForegroundColor, ResetColor},
};

struct CommandHistory 
{
    commands: VecDeque<String>,
    max_size: usize,
    current_index: Option<usize>,
    // I want to do something extra with history, idea for now
    filtered_commands: Vec<String>,
    suggestion_index: usize,
}

impl CommandHistory 
{
    fn new(max_size: usize) -> Self {
        CommandHistory {
            commands: VecDeque::with_capacity(max_size),
            max_size,
            current_index: None,
            filtered_commands: Vec::new(),
            suggestion_index: 0,
        }
    }

    // TODO: can we make the largest size 2^n and use the trick ?
    fn add(&mut self, command: String) {
        // If previous command is the same dont bother
        if self.commands.front().map_or(true, |last| *last != command) {
            if self.commands.len() == self.max_size {
                self.commands.pop_back();
            }
            self.commands.push_front(command);
        }
        self.current_index = None;
    }

    // Initial implementation just look at the start of all commands 
    // TODO: find fuzzy search library
    fn filter_commands(&mut self, start: &str)
    {
        self.filtered_commands = self.commands
            .iter()
            .filter(|cmd| cmd.starts_with(start))
            .cloned()
            .collect();

        self.suggestion_index = 0;
    }

    fn get_suggestions(&self) -> &[String] {
        &self.filtered_commands
    }

    fn previous(&mut self) -> Option<String> {
        if self.commands.is_empty() {
            return None;
        }

        self.current_index = Some(match self.current_index {
            None => 0,
            Some(idx) if idx + 1 < self.commands.len() => idx + 1,
            Some(_) => return None,
        });

        self.current_index.map(|idx| self.commands[idx].clone())
    }

    fn next(&mut self) -> Option<String> {
        match self.current_index {
            None => None,
            Some(0) => {
                self.current_index = None;
                None
            }
            Some(idx) => {
                self.current_index = Some(idx - 1);
                Some(self.commands[idx - 1].clone())
            }
        }
    }
}

/*
    The idea is to simply have a list of recent commmands drop down while writing commands    
 */
fn display_suggestions(history: &CommandHistory, current_input: &str, cursor_pos: usize) 
{
    let suggestions = history.get_suggestions();
    if suggestions.is_empty() {
        return;
    }

    if let Err(_) = execute!(stdout(), cursor::SavePosition) {
        return;
    }

    // Clear previous suggestions (up to 5 lines)
    for i in 1..=5 {
        if let Err(_) = execute!(
            stdout(),
            cursor::MoveDown(1),
            cursor::MoveToColumn(0),
            terminal::Clear(ClearType::CurrentLine),
        ) {
            return;
        }
    }

    // Go back to first suggestion line
    if let Err(_) = execute!(stdout(), cursor::RestorePosition, cursor::MoveDown(1)) {
        return;
    }

    // Display up to 5 suggestions, with the current selection highlighted
    for (i, suggestion) in suggestions.iter().take(5).enumerate() 
    {
        if let Err(_) = execute!(
            stdout(),
            cursor::MoveToColumn(0)
        ) {
            return;
        }

        // Highlight the current suggestion
        if i == history.suggestion_index {
            if let Err(_) = execute!(stdout(), SetForegroundColor(Color::Green)) {
                return;
            }
            print!("> {}", suggestion);
        } else {
            if let Err(_) = execute!(stdout(), SetForegroundColor(Color::DarkGrey)) {
                return;
            }
            print!(">  {}", suggestion);
        }
        if let Err(_) = execute!(stdout(), ResetColor) {
            return;
        }

        // Move to next line for next suggestion
        if i < suggestions.len() - 1 && i < 4 {
            if let Err(_) = execute!(stdout(), cursor::MoveDown(1)) {
                return;
            }
        }
    }

    // Reset color and restore cursor to original position
    if let Err(_) = execute!(stdout(), ResetColor, cursor::RestorePosition) {
        return;
    }
    let _ = stdout().flush();
}

fn clear_suggestions() 
{
    if let Err(_) = execute!(stdout(), cursor::SavePosition) {
        return;
    }

    // Clear the next 5 lines (maximum number of suggestions)
    for _ in 0..5 {
        if let Err(_) = execute!(
            stdout(),
            cursor::MoveDown(1),
            cursor::MoveToColumn(0),
            terminal::Clear(ClearType::CurrentLine)
        ) {
            return;
        }
    }

    if let Err(_) = execute!(stdout(), cursor::RestorePosition) {
        return;
    }
    let _ = stdout().flush();
}

fn redraw_line(hostname: &str, input: &str, cursor_pos: usize) 
{
    if let Err(_) = execute!(
        stdout(),
        cursor::Hide, // remove flickering
        cursor::MoveToColumn(0),
        terminal::Clear(ClearType::CurrentLine)
    ) {
        return;
    }
    print!("{}> {}", hostname, input);

    if let Err(_) = execute!(
        stdout(),
        cursor::MoveToColumn((hostname.len() + 2 + cursor_pos) as u16),
        cursor::Show
    ) {
        return;
    }
    let _ = stdout().flush();
}


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

    let mut history = CommandHistory::new(64);

    // enable terminal raw mode so we can read incomplete commands
    // and do suggestions and cool stuff, but now we have to do 
    // everything ourselves
    if let Err(_) = terminal::enable_raw_mode() {
        eprintln!("Failed to enable raw mode");
        return;
    }

    loop 
    {
        print!("{}> ", hostname);
        if let Err(_) = stdout().flush() {
            continue;
        }

        let mut input = String::new();
        let mut cursor_pos = 0;

        loop
        {
            match event::read() 
            {
                Ok(Event::Key(KeyEvent { code, modifiers, .. })) => {

                    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
                        println!("^C");
                        std::process::exit(0);
                    }
                    match code {
                        KeyCode::Enter => {
                            clear_suggestions(); // Clear suggestion lines before printing newline
                            println!();
                            break;
                        }
                        // we will handle each key on our own now
                        KeyCode::Char(c) => {
                            input.push(c);
                            print!("{}", c);
                            cursor_pos += 1;
                            history.filter_commands(&input);
                            redraw_line(&hostname, &input, cursor_pos);
                        }
                        KeyCode::Backspace => {
                            if cursor_pos > 0 {
                                cursor_pos -= 1;
                                input.remove(cursor_pos);
                                history.filter_commands(&input);
                                redraw_line(&hostname, &input, cursor_pos);
                            }
                        }
                        KeyCode::Delete => {
                            if cursor_pos < input.len() {
                                input.remove(cursor_pos);
                                history.filter_commands(&input);
                                redraw_line(&hostname, &input, cursor_pos);
                            }
                        }
                        KeyCode::Left => {
                            if cursor_pos > 0 {
                                cursor_pos -= 1;
                                if let Err(_) = execute!(stdout(), cursor::MoveLeft(1)) {
                                    continue;
                                }
                            }
                        }
                        KeyCode::Right => {
                            if cursor_pos < input.len() {
                                cursor_pos += 1;
                                if let Err(_) = execute!(stdout(), cursor::MoveRight(1)) {
                                    continue;
                                }
                            }
                        }
                        KeyCode::Home => {
                            cursor_pos = 0;
                            if let Err(_) = execute!(
                                stdout(),
                                cursor::MoveToColumn((hostname.len() + 2) as u16)
                            ) {
                                continue;
                            }
                        }
                        KeyCode::End => {
                            cursor_pos = input.len();
                            if let Err(_) = execute!(
                                stdout(),
                                cursor::MoveToColumn((hostname.len() + 2 + cursor_pos) as u16)
                            ) {
                                continue;
                            }
                        }
                        KeyCode::Tab => {
                            let suggestions = history.get_suggestions();
                            if !suggestions.is_empty() {
                                if let Err(_) = execute!(
                                    stdout(),
                                    cursor::Hide,
                                    cursor::MoveToColumn(0),
                                    terminal::Clear(ClearType::CurrentLine)
                                ) {
                                    continue;
                                }
                                
                                // Get the current suggestion before incrementing the index
                                let current_suggestion = suggestions[history.suggestion_index].clone();
                                
                                // Update index for next tab press
                                history.suggestion_index = (history.suggestion_index + 1) % suggestions.len();
                                
                                // Replace with current suggestion
                                input = current_suggestion;
                                print!("{}> {}", hostname, input);
                                cursor_pos = input.len();
                                
                                if let Err(_) = execute!(stdout(), cursor::Show) {
                                    continue;
                                }
                            }
                        }
                        _ => {}
                    }
                    
                    let _ = stdout().flush();
                    display_suggestions(&history, &input, cursor_pos);
                }
                Ok(Event::Mouse(_)) => {}, // Ignore mouse events
                Ok(Event::Resize(_, _)) => {
                    redraw_line(&hostname, &input, cursor_pos);
                }, // Handle terminal resize if needed
                Ok(Event::FocusGained) => {}, // Ignore focus events
                Ok(Event::FocusLost) => {}, // Ignore focus events
                Ok(Event::Paste(_)) => {}, // Ignore paste events for now
                Err(_) => continue,
            }
        }

        let mut input = input.trim().to_string();

        if !input.is_empty() {
            history.add(input.clone());
        }
        history.add(input.to_string());

    
        let mut tokens = input.split_whitespace(); 
        let cmd = match tokens.next() {
            Some(c) => c,
            None => continue, // Skip empty input
        };
        let args = tokens;

        /*
            Some commands have to be built into the shell program 
            itself because they cannot work if they are external.
            `cd` is one such since if it were external, it could only change 
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
            "history" => {
                for (index, command) in history.commands.iter().enumerate() {
                    println!("{}\t{}", index + 1, command);
                }
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