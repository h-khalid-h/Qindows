//! # Q-Kit — Declarative Shader-Native Component Framework
//!
//! The SDK for building Qindows apps (Section 4.4). Developers
//! describe state-machines that compile directly into the GPU
//! pipeline. Animations are physical properties baked into the
//! kernel compositor.
//!
//! Features:
//! - Declarative component tree (like SwiftUI/React)
//! - GPU-native rendering via SDF shaders
//! - Physics-based animations (mass, friction, elasticity)
//! - Material system (Glass, Solid, Gradient)
//! - Automatic accessibility metadata

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Component types.
#[derive(Debug, Clone, PartialEq)]
pub enum Component {
    /// Text label
    Text { content: String, font_size: f32, color: u32 },
    /// Clickable button
    Button { label: String, style: ButtonStyle, action_id: u64 },
    /// Container with children
    Container { layout: Layout, children: Vec<Component> },
    /// Image (SDF-rendered)
    Image { asset_id: u64, width: f32, height: f32 },
    /// Spacer
    Spacer { size: f32 },
    /// Divider line
    Divider { thickness: f32, color: u32 },
    /// Scrollable area
    Scroll { child: Box<Component>, direction: ScrollDir },
    /// Input field
    Input { placeholder: String, value: String, input_id: u64 },
    /// Toggle switch
    Toggle { label: String, on: bool, toggle_id: u64 },
    /// Slider
    Slider { min: f32, max: f32, value: f32, slider_id: u64 },
}

/// Layout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Vertical stack
    VStack,
    /// Horizontal stack
    HStack,
    /// Centered
    Center,
    /// Layered (Z-axis)
    ZStack,
    /// Grid
    Grid { cols: u32 },
}

/// Button style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Frosted glass
    GlassMorph,
    /// Solid color
    Solid,
    /// Outlined
    Outlined,
    /// Text-only
    TextOnly,
}

/// Scroll direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDir {
    Vertical,
    Horizontal,
    Both,
}

/// Material for backgrounds.
#[derive(Debug, Clone, PartialEq)]
pub enum Material {
    /// Frosted glass
    Glass { blur: f32, tint: u32 },
    /// Solid color
    Solid(u32),
    /// Linear gradient
    Gradient { from: u32, to: u32, angle: f32 },
    /// Transparent
    Transparent,
}

/// Physics-based animation properties.
#[derive(Debug, Clone)]
pub struct PhysicsAnim {
    /// Animation type
    pub anim_type: AnimType,
    /// Mass (affects momentum)
    pub mass: f32,
    /// Friction (damping)
    pub friction: f32,
    /// Elasticity (bounce)
    pub elasticity: f32,
    /// Duration override (ms, 0 = physics-driven)
    pub duration_ms: u64,
}

/// Animation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimType {
    /// Scale on hover
    HoverScale,
    /// Press feedback
    PressDown,
    /// Appear transition
    FadeIn,
    /// Slide in from edge
    SlideIn,
    /// Elastic bounce
    Bounce,
    /// Spring to position
    Spring,
}

impl Default for PhysicsAnim {
    fn default() -> Self {
        PhysicsAnim {
            anim_type: AnimType::HoverScale,
            mass: 1.0,
            friction: 0.8,
            elasticity: 0.5,
            duration_ms: 0,
        }
    }
}

/// A Q-Kit app view.
#[derive(Debug, Clone)]
pub struct QView {
    /// Root component
    pub root: Component,
    /// Background material
    pub background: Material,
    /// Animations attached to component IDs
    pub animations: BTreeMap<u64, PhysicsAnim>,
    /// Current state values
    pub state: BTreeMap<String, StateValue>,
}

/// State values for reactive updates.
#[derive(Debug, Clone, PartialEq)]
pub enum StateValue {
    Bool(bool),
    Int(i64),
    Float(f32),
    Str(String),
}

/// App manifest for Q-Kit apps.
#[derive(Debug, Clone)]
pub struct AppManifest {
    /// App ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Entry Wasm module
    pub entry: String,
    /// Required capabilities
    pub capabilities: Vec<String>,
    /// Sentinel priority
    pub priority: String,
    /// Energy limit
    pub energy_limit: String,
}

/// Q-Kit framework statistics.
#[derive(Debug, Clone, Default)]
pub struct QKitStats {
    pub views_built: u64,
    pub components_rendered: u64,
    pub state_updates: u64,
    pub animations_started: u64,
}

/// The Q-Kit Framework.
pub struct QKit {
    /// Registered app views
    pub views: BTreeMap<u64, QView>,
    /// App manifests
    pub manifests: BTreeMap<String, AppManifest>,
    /// Next view ID
    next_view_id: u64,
    /// Statistics
    pub stats: QKitStats,
}

impl QKit {
    pub fn new() -> Self {
        QKit {
            views: BTreeMap::new(),
            manifests: BTreeMap::new(),
            next_view_id: 1,
            stats: QKitStats::default(),
        }
    }

    /// Build a new view from a component tree.
    pub fn build_view(&mut self, root: Component, background: Material) -> u64 {
        let id = self.next_view_id;
        self.next_view_id += 1;

        self.views.insert(id, QView {
            root,
            background,
            animations: BTreeMap::new(),
            state: BTreeMap::new(),
        });

        self.stats.views_built += 1;
        id
    }

    /// Attach a physics animation to a component action.
    pub fn add_animation(&mut self, view_id: u64, action_id: u64, anim: PhysicsAnim) {
        if let Some(view) = self.views.get_mut(&view_id) {
            view.animations.insert(action_id, anim);
            self.stats.animations_started += 1;
        }
    }

    /// Update state in a view (triggers re-render).
    pub fn set_state(&mut self, view_id: u64, key: &str, value: StateValue) {
        if let Some(view) = self.views.get_mut(&view_id) {
            view.state.insert(String::from(key), value);
            self.stats.state_updates += 1;
        }
    }

    /// Get state value.
    pub fn get_state(&self, view_id: u64, key: &str) -> Option<&StateValue> {
        self.views.get(&view_id)?.state.get(key)
    }

    /// Count components in a tree (recursive).
    pub fn count_components(component: &Component) -> usize {
        match component {
            Component::Container { children, .. } => {
                1 + children.iter().map(Self::count_components).sum::<usize>()
            }
            Component::Scroll { child, .. } => 1 + Self::count_components(child),
            _ => 1,
        }
    }

    /// Register an app manifest.
    pub fn register_app(&mut self, manifest: AppManifest) {
        self.manifests.insert(manifest.id.clone(), manifest);
    }
}
