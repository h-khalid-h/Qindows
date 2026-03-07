//! # Synapse Semantic Router
//!
//! Routes natural language intent to the appropriate Qindows subsystem
//! or application using semantic similarity, bypassing rigid regex rules (Section 3.5).
//!
//! Features:
//! - Intent vectorization
//! - Similarity thresholding to prevent hallucinated routing
//! - Fallback to generic web search / LLM consultation
//! - Route execution bridging (invokes Prism/Aether/Nexus)

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use crate::embeddings::{Embedding, EmbeddingIndex, ContentType};

/// Action route destinations within Qindows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteDestination {
    /// Open a Prism file or folder
    PrismOpen(u64),
    /// Search Prism for files/content
    PrismSearch,
    /// Launch or focus an application via Aether
    AetherApp(String),
    /// Change system settings (Network, Display, Audio)
    SystemSettings(String),
    /// Send an email or message via Nexus
    NexusCommunicate,
    /// Fallback: Ask generalized LLM knowledge
    GeneralKnowledge,
    /// Action not understood
    Unknown,
}

/// A registered route that Synapse can take.
#[derive(Debug, Clone)]
pub struct RegisteredRoute {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub destination: RouteDestination,
    /// Reference embedding representing the "ideal" query for this route
    pub signature: Embedding,
}

/// The Semantic Router.
pub struct SemanticRouter {
    pub routes: Vec<RegisteredRoute>,
    pub fallback_threshold: f32,
    pub index: EmbeddingIndex,
    next_route_id: u64,
}

impl SemanticRouter {
    pub fn new(index: EmbeddingIndex) -> Self {
        SemanticRouter {
            routes: Vec::new(),
            // Minimum cosine similarity required to trigger a route.
            // If the best match is below this, fallback to GeneralKnowledge.
            fallback_threshold: 0.65,
            index,
            next_route_id: 1,
        }
    }

    /// Register a new system route.
    pub fn register_route(&mut self, name: &str, description: &str, destination: RouteDestination) -> u64 {
        let id = self.next_route_id;
        self.next_route_id += 1;

        // Create an embedding signature based on the description (which acts as the prompt)
        let signature = self.index.embed_text(description, id, ContentType::Code);

        self.routes.push(RegisteredRoute {
            id,
            name: String::from(name),
            description: String::from(description),
            destination,
            signature,
        });

        id
    }

    /// Bootstraps the default Qindows core routes.
    pub fn bootstrap_core_routes(&mut self) {
        self.register_route(
            "file_search",
            "find open search locate files documents photos folders pdf word excel",
            RouteDestination::PrismSearch,
        );
        self.register_route(
            "send_message",
            "send email message text sms slack team chat write to contact",
            RouteDestination::NexusCommunicate,
        );
        self.register_route(
            "wifi_settings",
            "connect wifi network internet disconnect airplane mode bluetooth",
            RouteDestination::SystemSettings(String::from("network")),
        );
        self.register_route(
            "display_settings",
            "brightness screen resolution monitor dark mode light mode theme",
            RouteDestination::SystemSettings(String::from("display")),
        );
        self.register_route(
            "audio_settings",
            "volume mute quiet loud speaker microphone sound",
            RouteDestination::SystemSettings(String::from("audio")),
        );
    }

    /// Route a natural language query to a destination.
    pub fn route_query(&self, query_text: &str) -> (RouteDestination, f32) {
        // Compute embedding for the incoming user query
        let query_embed = self.index.embed_text(query_text, 0, ContentType::Message);

        let mut best_score: f32 = -1.0;
        let mut best_route: Option<&RegisteredRoute> = None;

        // Compare against all registered route signatures
        for route in &self.routes {
            let score = query_embed.similarity(&route.signature);
            if score > best_score {
                best_score = score;
                best_route = Some(route);
            }
        }

        if let Some(route) = best_route {
            if best_score >= self.fallback_threshold {
                return (route.destination.clone(), best_score);
            }
        }

        // If no route crossed the confidence threshold, fallback.
        (RouteDestination::GeneralKnowledge, best_score.max(0.0))
    }

    /// Dynamic app routing. Given a list of installed apps, finds the best match.
    pub fn route_app_launch(&self, app_list: &[&str], query_text: &str) -> Option<String> {
        let query_embed = self.index.embed_text(query_text, 0, ContentType::Message);
        
        let mut best_score: f32 = -1.0;
        let mut best_app: Option<String> = None;

        for app in app_list {
            // Embed the app name to see if it semantically matches the query
            let app_embed = self.index.embed_text(app, 0, ContentType::Code);
            let score = query_embed.similarity(&app_embed);
            
            if score > best_score {
                best_score = score;
                best_app = Some(String::from(*app));
            }
        }

        if best_score > 0.70 { // Stricter threshold for app launching
            best_app
        } else {
            None
        }
    }
}
