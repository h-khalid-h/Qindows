//! # Chimera DirectX Compatibility Shim
//!
//! Translates Direct3D 9/11 API calls from legacy Win32 apps into
//! Aether's GPU command buffer format. This lets classic Windows
//! games and multimedia apps render inside Q-Silos without native
//! DirectX hardware support.
//!
//! Architecture:
//!   Win32 app → D3D9/D3D11 API → this shim → Aether GPU commands
//!
//! Limitations: shader model 3.0 max, fixed-function pipeline only,
//! no tessellation or compute shaders.

extern crate alloc;

use alloc::vec::Vec;

/// Supported DirectX feature levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum D3dFeatureLevel {
    /// Direct3D 9.0c (shader model 3.0)
    D3d9,
    /// Direct3D 10.0 (shader model 4.0, limited)
    D3d10,
    /// Direct3D 11.0 (shader model 5.0, limited)
    D3d11,
}

impl D3dFeatureLevel {
    pub fn name(&self) -> &'static str {
        match self {
            D3dFeatureLevel::D3d9  => "Direct3D 9.0c",
            D3dFeatureLevel::D3d10 => "Direct3D 10.0",
            D3dFeatureLevel::D3d11 => "Direct3D 11.0",
        }
    }

    pub fn shader_model(&self) -> &'static str {
        match self {
            D3dFeatureLevel::D3d9  => "3.0",
            D3dFeatureLevel::D3d10 => "4.0",
            D3dFeatureLevel::D3d11 => "5.0",
        }
    }
}

/// A GPU resource handle (texture, buffer, etc.)
pub type ResourceHandle = u64;

/// Vertex element formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexFormat {
    Float1,
    Float2,
    Float3,
    Float4,
    Color4,    // D3DCOLOR: ARGB packed u32
    UByte4,
    Short2,
    Short4,
}

impl VertexFormat {
    pub fn size_bytes(&self) -> usize {
        match self {
            VertexFormat::Float1 => 4,
            VertexFormat::Float2 => 8,
            VertexFormat::Float3 => 12,
            VertexFormat::Float4 => 16,
            VertexFormat::Color4 => 4,
            VertexFormat::UByte4 => 4,
            VertexFormat::Short2 => 4,
            VertexFormat::Short4 => 8,
        }
    }
}

/// Vertex element semantic (what the data represents).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexSemantic {
    Position,
    Normal,
    TexCoord(u8),  // Index (0-7)
    Color(u8),     // Index (0-1)
    Tangent,
    Binormal,
    BlendWeight,
    BlendIndices,
}

/// A vertex element declaration.
#[derive(Debug, Clone)]
pub struct VertexElement {
    pub stream: u8,
    pub offset: u16,
    pub format: VertexFormat,
    pub semantic: VertexSemantic,
}

/// Texture format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFormat {
    Rgba8,       // R8G8B8A8_UNORM
    Bgra8,       // B8G8R8A8_UNORM (D3D default)
    Dxt1,        // BC1 compressed
    Dxt3,        // BC2 compressed
    Dxt5,        // BC3 compressed
    R32Float,    // R32_FLOAT (depth)
    D24S8,       // Depth24_Stencil8
}

impl TextureFormat {
    pub fn bytes_per_pixel(&self) -> f32 {
        match self {
            TextureFormat::Rgba8    => 4.0,
            TextureFormat::Bgra8    => 4.0,
            TextureFormat::Dxt1     => 0.5,  // 8 bytes per 4x4 block
            TextureFormat::Dxt3     => 1.0,
            TextureFormat::Dxt5     => 1.0,
            TextureFormat::R32Float => 4.0,
            TextureFormat::D24S8    => 4.0,
        }
    }
}

/// Primitive topology (what the vertex data represents).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveTopology {
    PointList,
    LineList,
    LineStrip,
    TriangleList,
    TriangleStrip,
    TriangleFan,  // D3D9 only
}

/// Blend mode for alpha blending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    Opaque,
    AlphaBlend,
    Additive,
    Multiply,
    Custom { src: BlendFactor, dst: BlendFactor },
}

/// Blend factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendFactor {
    Zero,
    One,
    SrcAlpha,
    InvSrcAlpha,
    DstAlpha,
    InvDstAlpha,
    SrcColor,
    InvSrcColor,
}

/// Depth/stencil state.
#[derive(Debug, Clone, Copy)]
pub struct DepthStencilState {
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_func: CompareFunc,
    pub stencil_enable: bool,
    pub stencil_ref: u8,
    pub stencil_mask: u8,
}

impl Default for DepthStencilState {
    fn default() -> Self {
        DepthStencilState {
            depth_test: true,
            depth_write: true,
            depth_func: CompareFunc::Less,
            stencil_enable: false,
            stencil_ref: 0,
            stencil_mask: 0xFF,
        }
    }
}

/// Comparison functions (for depth/stencil tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareFunc {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

/// A GPU command emitted by the shim (consumed by Aether renderer).
#[derive(Debug, Clone)]
pub enum GpuCommand {
    /// Clear render target
    Clear { color: [f32; 4], depth: f32, stencil: u8, flags: u8 },
    /// Set vertex buffer
    SetVertexBuffer { handle: ResourceHandle, stride: u32, offset: u32 },
    /// Set index buffer
    SetIndexBuffer { handle: ResourceHandle, format_16bit: bool },
    /// Set texture on a slot
    SetTexture { slot: u8, handle: ResourceHandle },
    /// Set viewport
    SetViewport { x: u16, y: u16, width: u16, height: u16, min_z: f32, max_z: f32 },
    /// Set scissor rect
    SetScissor { x: u16, y: u16, width: u16, height: u16 },
    /// Set world/view/projection matrix
    SetMatrix { matrix_type: MatrixType, values: [f32; 16] },
    /// Set blend mode
    SetBlendMode(BlendMode),
    /// Set depth/stencil state
    SetDepthStencil(DepthStencilState),
    /// Draw primitives (non-indexed)
    Draw { topology: PrimitiveTopology, start_vertex: u32, vertex_count: u32 },
    /// Draw indexed primitives
    DrawIndexed { topology: PrimitiveTopology, base_vertex: i32, index_count: u32, start_index: u32 },
    /// Present (swap chain flip)
    Present,
}

/// Matrix types for the fixed-function pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixType {
    World,
    View,
    Projection,
    Texture(u8),
}

/// A GPU resource tracked by the shim.
#[derive(Debug, Clone)]
pub struct GpuResource {
    pub handle: ResourceHandle,
    pub resource_type: ResourceType,
    pub size_bytes: u64,
    pub format: Option<TextureFormat>,
    pub width: u32,
    pub height: u32,
    pub ref_count: u32,
}

/// Resource types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    VertexBuffer,
    IndexBuffer,
    Texture2D,
    RenderTarget,
    DepthStencil,
    ConstantBuffer,
}

/// D3D shim statistics.
#[derive(Debug, Clone, Default)]
pub struct D3dStats {
    pub draw_calls: u64,
    pub triangles_drawn: u64,
    pub texture_switches: u64,
    pub state_changes: u64,
    pub frames_presented: u64,
    pub resources_created: u64,
    pub resources_destroyed: u64,
    pub gpu_memory_used: u64,
}

/// The DirectX Compatibility Shim.
pub struct D3dShim {
    /// Supported feature level
    pub feature_level: D3dFeatureLevel,
    /// Silo ID this shim belongs to
    pub silo_id: u64,
    /// Pending GPU commands (flushed to Aether on Present)
    pub command_buffer: Vec<GpuCommand>,
    /// Tracked resources
    pub resources: Vec<GpuResource>,
    /// Next resource handle
    next_handle: ResourceHandle,
    /// Current pipeline state
    pub current_blend: BlendMode,
    pub current_depth: DepthStencilState,
    /// Stats
    pub stats: D3dStats,
    /// Backbuffer dimensions
    pub backbuffer_width: u32,
    pub backbuffer_height: u32,
}

impl D3dShim {
    /// Create a new D3D shim for a Silo.
    pub fn new(silo_id: u64, feature_level: D3dFeatureLevel, width: u32, height: u32) -> Self {
        D3dShim {
            feature_level,
            silo_id,
            command_buffer: Vec::new(),
            resources: Vec::new(),
            next_handle: 1,
            current_blend: BlendMode::Opaque,
            current_depth: DepthStencilState::default(),
            stats: D3dStats::default(),
            backbuffer_width: width,
            backbuffer_height: height,
        }
    }

    // ─── Resource Management ────────────────────────────────────────

    /// Create a vertex buffer.
    pub fn create_vertex_buffer(&mut self, size: u64) -> ResourceHandle {
        self.create_resource(ResourceType::VertexBuffer, size, 0, 0, None)
    }

    /// Create an index buffer.
    pub fn create_index_buffer(&mut self, size: u64) -> ResourceHandle {
        self.create_resource(ResourceType::IndexBuffer, size, 0, 0, None)
    }

    /// Create a 2D texture.
    pub fn create_texture_2d(
        &mut self, width: u32, height: u32, format: TextureFormat,
    ) -> ResourceHandle {
        let size = (width as f32 * height as f32 * format.bytes_per_pixel()) as u64;
        self.create_resource(ResourceType::Texture2D, size, width, height, Some(format))
    }

    /// Create a render target.
    pub fn create_render_target(
        &mut self, width: u32, height: u32, format: TextureFormat,
    ) -> ResourceHandle {
        let size = (width as f32 * height as f32 * format.bytes_per_pixel()) as u64;
        self.create_resource(ResourceType::RenderTarget, size, width, height, Some(format))
    }

    fn create_resource(
        &mut self, resource_type: ResourceType, size: u64,
        width: u32, height: u32, format: Option<TextureFormat>,
    ) -> ResourceHandle {
        let handle = self.next_handle;
        self.next_handle += 1;

        self.resources.push(GpuResource {
            handle,
            resource_type,
            size_bytes: size,
            format,
            width,
            height,
            ref_count: 1,
        });

        self.stats.resources_created += 1;
        self.stats.gpu_memory_used += size;
        handle
    }

    /// Release a resource.
    pub fn release_resource(&mut self, handle: ResourceHandle) {
        if let Some(pos) = self.resources.iter().position(|r| r.handle == handle) {
            let res = self.resources.remove(pos);
            self.stats.gpu_memory_used = self.stats.gpu_memory_used.saturating_sub(res.size_bytes);
            self.stats.resources_destroyed += 1;
        }
    }

    // ─── Draw Commands ──────────────────────────────────────────────

    /// Clear the render target.
    pub fn clear(&mut self, color: [f32; 4], depth: f32, stencil: u8) {
        self.command_buffer.push(GpuCommand::Clear {
            color, depth, stencil, flags: 0x07, // Color + depth + stencil
        });
    }

    /// Set a vertex buffer.
    pub fn set_vertex_buffer(&mut self, handle: ResourceHandle, stride: u32) {
        self.command_buffer.push(GpuCommand::SetVertexBuffer {
            handle, stride, offset: 0,
        });
        self.stats.state_changes += 1;
    }

    /// Set an index buffer.
    pub fn set_index_buffer(&mut self, handle: ResourceHandle, format_16bit: bool) {
        self.command_buffer.push(GpuCommand::SetIndexBuffer { handle, format_16bit });
        self.stats.state_changes += 1;
    }

    /// Bind a texture to a sampler slot.
    pub fn set_texture(&mut self, slot: u8, handle: ResourceHandle) {
        self.command_buffer.push(GpuCommand::SetTexture { slot, handle });
        self.stats.texture_switches += 1;
    }

    /// Set a transform matrix.
    pub fn set_transform(&mut self, matrix_type: MatrixType, values: [f32; 16]) {
        self.command_buffer.push(GpuCommand::SetMatrix { matrix_type, values });
        self.stats.state_changes += 1;
    }

    /// Set blend mode.
    pub fn set_blend_mode(&mut self, mode: BlendMode) {
        self.current_blend = mode;
        self.command_buffer.push(GpuCommand::SetBlendMode(mode));
        self.stats.state_changes += 1;
    }

    /// Set viewport.
    pub fn set_viewport(&mut self, x: u16, y: u16, width: u16, height: u16) {
        self.command_buffer.push(GpuCommand::SetViewport {
            x, y, width, height, min_z: 0.0, max_z: 1.0,
        });
    }

    /// Draw non-indexed primitives.
    pub fn draw(
        &mut self, topology: PrimitiveTopology, start: u32, count: u32,
    ) {
        let tris = match topology {
            PrimitiveTopology::TriangleList  => count / 3,
            PrimitiveTopology::TriangleStrip => count.saturating_sub(2),
            PrimitiveTopology::TriangleFan   => count.saturating_sub(2),
            _ => 0,
        };

        self.command_buffer.push(GpuCommand::Draw {
            topology, start_vertex: start, vertex_count: count,
        });
        self.stats.draw_calls += 1;
        self.stats.triangles_drawn += tris as u64;
    }

    /// Draw indexed primitives.
    pub fn draw_indexed(
        &mut self, topology: PrimitiveTopology,
        base_vertex: i32, index_count: u32, start_index: u32,
    ) {
        let tris = match topology {
            PrimitiveTopology::TriangleList  => index_count / 3,
            PrimitiveTopology::TriangleStrip => index_count.saturating_sub(2),
            _ => 0,
        };

        self.command_buffer.push(GpuCommand::DrawIndexed {
            topology, base_vertex, index_count, start_index,
        });
        self.stats.draw_calls += 1;
        self.stats.triangles_drawn += tris as u64;
    }

    /// Present (end frame, flush command buffer to Aether).
    pub fn present(&mut self) -> Vec<GpuCommand> {
        self.command_buffer.push(GpuCommand::Present);
        self.stats.frames_presented += 1;
        core::mem::take(&mut self.command_buffer)
    }

    /// Get current GPU memory usage.
    pub fn gpu_memory_mb(&self) -> f64 {
        self.stats.gpu_memory_used as f64 / (1024.0 * 1024.0)
    }
}
