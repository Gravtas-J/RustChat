use dotenv::dotenv;
use reqwest::{Client, header};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::{env, fs};
use std::io::{self, Write};
use std::path::Path;
use tokio;
use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};
// Assuming a difflib equivalent exists in Rust or you have implemented a basic version
use difflib::sequencematcher::SequenceMatcher;

// Added for the profile update functionality
#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}
// Utility function to read the initial prompt from a file
fn read_initial_prompt(file_path: &str) -> Result<String, io::Error> {
    fs::read_to_string(file_path)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");

    println!("Welcome to the Rust Chatbot!");
    println!("Do you want verbose logging? (yes/no)");
    let mut verbose_input = String::new();
    io::stdin().read_line(&mut verbose_input)?;
    let verbose = verbose_input.trim().eq_ignore_ascii_case("yes");

    // Read the initial system prompt from the file
    let file_prompt = read_initial_prompt("system_prompts/prompt.md").unwrap_or_else(|err| {
        eprintln!("Failed to read initial prompt from file: {}", err);
        String::new() // Fallback to an empty string or provide a default prompt
    });

    let mut conversation_log: Vec<Value> = Vec::new();

    // If there's an initial prompt, add it to the conversation log as a system message
    if !file_prompt.is_empty() {
        conversation_log.push(json!({"role": "system", "content": file_prompt}));
    }

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("You: ");
        stdout.flush()?;
        let mut input = String::new();
        stdin.read_line(&mut input)?;

        let user_input = input.trim();
        if !user_input.is_empty() {
            conversation_log.push(json!({"role": "user", "content": user_input}));
        }

        let (tx, rx) = oneshot::channel();
        let animation_handle = tokio::spawn(async move {
            animate_thinking(rx).await;
        });

        let response = query_gpt(&conversation_log, verbose).await?;

        let _ = tx.send(());
        let _ = animation_handle.await;

        print_response_character_by_character(&response).await;

        if !response.trim().is_empty() {
            conversation_log.push(json!({"role": "assistant", "content": response}));
        }

        // Call to update the profile after a response is generated
        let userprofile_path = "memories/userprofile.txt";
        let backup_userprofile_path = "memories/userprofile_backup.txt";
        update_profile(&api_key, userprofile_path, backup_userprofile_path).await?;
    }
}

// New update_profile function adapted to async and integrated with your existing program
async fn update_profile(api_key: &str, userprofile: &str, backup_userprofile: &str) -> Result<(), Box<dyn std::error::Error>> {
    let original_data = fs::read_to_string(Path::new(userprofile))?;

    let update_data = vec![
        ChatMessage {
            role: "system".to_string(),
            content: "Profile_check".to_string(), // Define Profile_check appropriately
        },
        // Ensure to replace "user_chat_log_content" with actual content
        ChatMessage {
            role: "user".to_string(),
            content: "user_chat_log_content".to_string(),
        },
    ];

    let client = Client::new();
    let response = client.post("https://api.openai.com/v1/chat/completions")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {}", api_key))
        .json(&json!({
            "model": "gpt-3.5-turbo-0125",
            "messages": update_data,
            "temperature": 0,
            "max_tokens": 4000
        }))
        .send()
        .await?;

    let response_body = response.json::<Value>().await?;
    let user_profile_updated = response_body["choices"][0]["message"]["content"].as_str().unwrap_or_default();

    let diff = SequenceMatcher::new(&original_data, user_profile_updated);
    let num_differences = diff.get_opcodes().iter().filter(|&&(tag, _, _, _, _)| tag != "equal").count();

    if num_differences > 200 {
        let restored_data = fs::read_to_string(Path::new(backup_userprofile))?;
        fs::write(Path::new(userprofile), restored_data)?;
    } else {
        fs::write(Path::new(userprofile), user_profile_updated)?;
    }

    Ok(())
}

async fn query_gpt(conversation_log: &[Value], verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
    let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
    let client = Client::new();

    // Ensure verbose logging is informative and correctly placed
    if verbose {
        println!("Conversation log for API request: {:?}", conversation_log);
    }

    // Correctly structured API request for the chat model
    let response = client.post("https://api.openai.com/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "model": "gpt-3.5-turbo", // Ensure you're using the correct model identifier
            "messages": conversation_log, // Pass the conversation log directly
        }))
        .send()
        .await?;

    // Check the response status after the call, before attempting to consume the response body
    if verbose {
        println!("Response status: {}", response.status());
    }

    // Assuming the response is successful, parse it
    if response.status().is_success() {
        let res: Value = response.json().await?;
        Ok(res["choices"].get(0).and_then(|choice| choice["message"]["content"].as_str()).unwrap_or_default().to_string())
    } else {
        // Handle error responses here
        let error_message = response.text().await?;
        Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, format!("API call failed: {}", error_message))))
    }
}


async fn animate_thinking(mut stop_signal: oneshot::Receiver<()>) {
    let mut dots = 0;
    loop {
        if stop_signal.try_recv().is_ok() {
            println!("\rThinking{} ", " ".repeat(6)); // Clear the line and add space for transition
            break;
        }

        if dots == 6 {
            print!("\rThinking{}", " ".repeat(6)); // Clear the dots visually
            dots = 0;
        } else {
            print!("\rThinking{}", ".".repeat(dots));
            dots += 1;
        }
        io::stdout().flush().unwrap();
        sleep(Duration::from_millis(100)).await;
    }
}
 
async fn print_response_character_by_character(response: &String) {
    print!("Bot: "); // Print the "Bot: " prefix before the response
    for c in response.chars() {
        print!("{}", c);
        io::stdout().flush().unwrap();
        sleep(Duration::from_millis(10)).await;
    }
    println!(); // Ensure the output ends on a new line
}
