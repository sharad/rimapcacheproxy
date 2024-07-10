extern crate imap;
extern crate tokio;

use std::collections::HashMap;
use imap::client::{Client, PlainSaslMechanism, SaslMechanism};
use tokio::net::TcpListener;
use tokio::stream::StreamExt;

struct ImapProxyServer {
    remote_server: String,
    remote_username: String,
    remote_password: String,
    remote_auth_method: SaslMechanism,
    local_port: u16,
    cache: HashMap<String, Vec<u8>>, // Mailbox name -> headers (consider expanding for message bodies)
}


fn main() {
    // Replace with your configuration values
    let remote_server = "remote_server.com";
    let remote_username = "username";
    let remote_password = "password";
    let remote_auth_method = PlainSaslMechanism; // Or other mechanism
    let local_port = 1433; // Common IMAP port, adjust if needed

    let mut server = ImapProxyServer::new(
        remote_server.to_string(),
        remote_username.to_string(),
        remote_password.to_string(),
        remote_auth_method,
        local_port,
    );

    if let Err(err) = server.run() {
        eprintln!("Error running server: {}", err);
    }
}

impl ImapProxyServer {
    fn new(
        remote_server: String,
        remote_username: String,
        remote_password: String,
        remote_auth_method: SaslMechanism,
        local_port: u16,
    ) -> Self {
        ImapProxyServer {
            remote_server,
            remote_username,
            remote_password,
            remote_auth_method,
            local_port,
            cache: HashMap::new(),
        }
    }


    async fn run(&mut self) -> Result<(), imap::Error> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.local_port)).await?;

        println!("IMAP Proxy Server listening on port {}", self.local_port);

        loop {
            let (mut stream, _) = listener.accept().await?;
            let mut remote_client = self.connect_to_remote().await?;
            let mut user_client = Client::new(stream)?;

            // Authenticate with remote server
            user_client.auth(&self.remote_auth_method, &[&self.remote_username, &self.remote_password])
                .await?;

            tokio::spawn(async move {
                loop {
                    let command = user_client.wait_tagged().await?;
                    if let Err(err) = self.handle_user_command(&mut remote_client, &command).await {
                        eprintln!("Error handling user command: {}", err);
                    }
                }
            });

            // Handle IDLE command separately for non-blocking behavior
            tokio::spawn(async move {
                loop {
                    let command = user_client.wait_tagged().await?;
                    if command.tag == "IDLE" {
                        self.handle_idle(&mut remote_client, &mut user_client).await?;
                        break;
                    }
                }

                // Exit IDLE loop when a non-IDLE command is received
            });

            // Wait for user and remote connections to close
            tokio::join!();
        }
    }


    async fn run(&mut self) -> Result<(), imap::Error> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.local_port)).await?;

        println!("IMAP Proxy Server listening on port {}", self.local_port);

        loop {
            let (mut stream, _) = listener.accept().await?;
            let mut remote_client = self.connect_to_remote().await?;
            let mut user_client = Client::new(stream)?;

            // Authenticate with remote server
            user_client.auth(&self.remote_auth_method, &[&self.remote_username, &self.remote_password])
                .await?;

            tokio::spawn(async move {
                loop {
                    let command = user_client.wait_tagged().await?;
                    if let Err(err) = self.handle_user_command(&mut remote_client, &command).await {
                        eprintln!("Error handling user command: {}", err);
                    }
                }
            });

            // Handle IDLE command separately for non-blocking behavior
            tokio::spawn(async move {
                loop {
                    let command = user_client.wait_tagged().await?;
                    if command.tag == "IDLE" {
                        self.handle_idle(&mut remote_client, &mut user_client).await?;
                        break;
                    }
                }

                // Exit IDLE loop when a non-IDLE command is received
            });

            // Wait for user and remote connections to close
            tokio::join!();
        }


        let update_interval = Duration::from_secs(60); // Update headers every 60 seconds

        loop {
            for mailbox in &["INBOX", "SENT"] { // Update headers for specific mailboxes
                if let Err(err) = self.update_mailbox_headers(mailbox).await {
                    eprintln!("Error updating headers for mailbox '{}': {}", mailbox, err);
                }
            }

            sleep(update_interval).await;
        }
    }

    async fn connect_to_remote(&self) -> Result<Client, imap::Error> {
        let client = Client::connect(self.remote_server.clone(), imap::tls::NoTls).await?;
        client.auth(&self.remote_auth_method, &[&self.remote_username, &self.remote_password])
            .await?;
        Ok(client)
    }


    async fn update_mailbox_headers(&mut self, mailbox: &str) -> Result<(), imap::Error> {
        let mut remote_client = self.connect_to_remote().await?;
        let response = remote_client.list(None, Some(mailbox)).await?;

        // Extract mailbox headers from the response
        let mailbox_data = response.get_mailbox(mailbox)
            .ok_or_else(|| imap::Error::MailboxNotFound(mailbox.to_string()))?;
        let headers = mailbox_data.to_owned(); // Convert to owned data

        // Update the cache with the new headers
        self.cache.insert(mailbox.to_string(), headers.to_vec());

        Ok(())
    }

    async fn handle_idle(&mut self, remote_client: &mut Client, user_client: &mut Client) -> Result<(), imap::Error> {
        let mut idle = remote_client.idle().await?;

        loop {
            let response = idle.wait().await?;
            match response {
                imap::response::Idle::Recent(recent) => {
                    // Handle new message notification (e.g., inform user client)
                    println!("New message received! Recent: {}", recent);
                    // Consider updating your cache if necessary
                },
                imap::response::Idle::Exists(exists) => {
                    // Handle mailbox exists notification (potential cache update)
                    println!("Mailbox exists notification: {}", exists);
                },
                imap::response::Idle::Flags(flags) => {
                    // Handle flags notification (potential cache update)
                    println!("Flags notification: {:?}", flags);
                },
                imap::response::Idle::Expunge(expunged) => {
                    // Handle expunge notification (invalidate cache entries)
                    println!("Message expunged: {}", expunged);
                    self.cache.remove(&expunged.to_string());
                },
                imap::response::Idle::Other(data) => {
                    // Handle other types of notifications
                    println!("Other IDLE notification: {}", data);
                },
            }

            // Check for non-IDLE commands from the user client
            if let Ok(command) = user_client.wait_tagged().await {
                if command.tag != "IDLE" {
                    break; // Exit IDLE loop when a non-IDLE command is received
                }
            }
        }

        Ok(())
    }

    async fn handle_user_command(&mut self, remote_client: &mut Client, command: &imap::command::Command) -> Result<Vec<u8>, imap::Error> {
        match command.tag {
            "READ" => {
                // Check cache for message body (consider expanding cache for bodies)
                if let Some(body) = self.cache.get(&command.arguments[0].to_string()) {
                    // Message body found in cache, return it
                    return Ok(body.to_vec());
                }

                // Message body not in cache, fetch from remote server and update cache
                let response = remote_client.fetch(&[command.arguments[0].clone()], false).await?;
                let body = response.get_body(&command.arguments[0])
                    .ok_or_else(|| imap::Error::InternalError("Message body not found in response"))?;
                self.cache.insert(command.arguments[0].to_string(), body.to_vec());
                Ok(body.to_vec())
            },
            "DELETE" => {
                // Relay DELETE command and data to remote server
                remote_client
                    .delete(&[command.arguments[0].clone()])
                    .await?;
                // Invalidate cache entry for the deleted message (if applicable)
                self.cache.remove(&command.arguments[0].to_string());
                Ok(vec![])
            },
            "LIST" => {
                // ... (consider caching mailbox list or fetching from remote server)
                let response = remote_client.list(None, None).await?;
                // Parse response and send relevant information to user client
                // ...
                Ok(vec![])
            },
            "SELECT" => {
                // Relay SELECT command to remote server
                remote_client.select(&command.arguments[0]).await?;
                Ok(vec![])
            },
            "SEARCH" => {
                // Relay SEARCH command to remote server
                // Parse search results and potentially update cache
                // ...
                Ok(vec![])
            },
            // "FETCH" (flags, criteria) => {
            //     // Relay FETCH command with flags and criteria to remote server
            //     // Handle potential partial fetches and cache updates
            //     // ...
            //     Ok(vec![])
            // },

            "FETCH" flags, criteria => {
                // Check if message body is requested
                let needs_body = criteria.iter().any(|crit| crit == &imap::criteria::Fetch::Body);

                if needs_body {
                    // Check cache for message body (if applicable)
                    let mailbox = &command.arguments[0]; // Assuming mailbox name is the first argument
                    if let Some(body) = self.cache.get(mailbox.to_string()) {
                        // Body found in cache, return it
                        return Ok(body.to_vec());
                    }
                }

                // Relay FETCH command to remote server (with partial fetch disabled for efficiency)
                let response = remote_client.fetch(criteria, false).await?;

                // Handle partial fetches and cache updates
                if needs_body {
                    let message = response.get_body(&command.arguments[0])
                        .ok_or_else(|| imap::Error::InternalError("Message body not found in response"))?;

                    // Cache the entire message body or specific parts depending on criteria
                    if criteria.iter().all(|crit| crit == &imap::criteria::Fetch::Body) {
                        self.cache.insert(mailbox.to_string(), message.to_vec());
                    } else {
                        // Handle caching specific body parts based on criteria (exercise for you)
                        // Consider using `message.get_envelope()`, `message.get_headers()`, or
                        // `message.get_body_section()` for selective caching
                    }

                    // Return the fetched message body
                    Ok(message.to_vec())
                } else {
                    // No body requested, return empty response (can be customized)
                    Ok(vec![])
                }
            },
            "EXPUNGE" => {
                // Relay EXPUNGE command to remote server
                // Invalidate cache entries for expunged messages
                // ...
                Ok(vec![])
            },

            // Additional commands (placeholders and explanations)
            "APPEND" (mailbox, flags, data) => {
                // Relay APPEND command with mailbox name, flags, and message data
                // to the remote server
                remote_client.append(mailbox, flags, data).await?;
                Ok(vec![])
            },
            "UID FETCH" (sequence_set, criteria) => {
                // Relay UID FETCH command with sequence set and criteria
                // to the remote server (potentially using cache)
                let response = remote_client.uid_fetch(sequence_set, criteria).await?;
                // Parse response and handle UID-specific data (e.g., UID of the message)
                // ...
                Ok(vec![])
            },
            "COPY" (source, destination) => {
                // Relay COPY command with source and destination mailbox names
                // to the remote server
                remote_client.copy(source, destination).await?;
                Ok(vec![])
            },
            "STORE" (sequence_set, flags, data) => {
                // Relay STORE command with sequence set, flags to set/modify, and data
                // to the remote server
                remote_client.store(sequence_set, flags, data).await?;
                Ok(vec![])
            },
            "SORT" (criteria, sort_keys) => {
                // Relay SORT command with search criteria and sorting keys
                // to the remote server (potentially using cached mailbox information)
                let response = remote_client.sort(criteria, sort_keys).await?;
                // Parse response and handle sorted message IDs
                // ...
                Ok(vec![])
            },
            "STATUS" (mailbox, items) => {
                // Relay STATUS command with mailbox name and items to get status for
                // to the remote server
                let response = remote_client.status(mailbox, items).await?;
                // Parse response and handle mailbox status information
                // ...
                Ok(vec![])
            },
            "NOOP" => {
                // Relay NOOP command to the remote server (does nothing)
                remote_client.noop().await?;
                Ok(vec![])
            },
            // Add placeholders for other IMAP commands as needed
            _ => Err(imap::Error::UnsupportedCommand(command.command.clone())),
        }
    }

}



