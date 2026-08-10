//! sample-harrier: スペースハリアー風、疑似3Dスケーリングスプライトの
//! ショートゲームプロトタイプ(Phase 0)。
//!
//! **正直な開示(現在のスコープ)**: これは`open-directx`の
//! `directx-graphics-window`(実ウィンドウ+Vulkanスワップチェーン+実
//! キーボード入力、DXBC->SPIR-V翻訳済みスプライトシェーダーを再利用)を
//! 土台とした最小限のプロトタイプ。実際のスペースハリアーが持つ
//! 「奥から迫ってくる敵の拡大縮小」を、1個の疑似3Dスケーリングスプライト
//! (自機の照準)で再現するところまで——地面のスクロール・複数の敵・
//! 得点・ステージ進行等は未実装(次回以降のロードマップ)。
//!
//! 内容: 640x480ウィンドウ。地面(緑の帯、画面下半分固定)+奥から迫って
//! くる敵スプライト(赤、Z値に応じて拡大・接近しループ)。矢印キー/
//! WASDで照準(黄色スプライト)を左右上下に移動、敵の拡大が最大に近い
//! (=接近しすぎた)タイミングで照準が重なっていればヒット判定して
//! 敵を奥へリセットする(得点はまだ加算しない、ヒットのログ出力のみ)。
//!
//! 実行方法: `cargo run --release`

use std::ffi::CString;

use ash::{vk, Entry};
use directx_graphics_vulkan::{Rgba8, TextureRgba8};
use directx_shader_translate::spirv_gen::{translate_sprite_pixel_shader, translate_sprite_vertex_shader};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use winit::event::{ElementState, Event, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

fn quad_vertices(dest_ndc: [f32; 4]) -> [Vertex; 6] {
    let [x0, y0, x1, y1] = dest_ndc;
    let tl = Vertex { pos: [x0, y0, 0.0], uv: [0.0, 0.0] };
    let tr = Vertex { pos: [x1, y0, 0.0], uv: [1.0, 0.0] };
    let br = Vertex { pos: [x1, y1, 0.0], uv: [1.0, 1.0] };
    let bl = Vertex { pos: [x0, y1, 0.0], uv: [0.0, 1.0] };
    [tl, tr, br, tl, br, bl]
}

/// ゲーム状態: 照準(自機)+奥から迫る敵1体+固定の地面帯。
struct GameState {
    reticle_x: f32,
    reticle_y: f32,
    up_held: bool,
    down_held: bool,
    left_held: bool,
    right_held: bool,
    /// 敵の「奥行き」。0.0(最も奥、最小サイズ)〜1.0(目前、最大サイズ)。
    enemy_depth: f32,
    enemy_x: f32,
    hits: u32,
}

impl GameState {
    fn new() -> Self {
        GameState {
            reticle_x: 0.0,
            reticle_y: 0.0,
            up_held: false,
            down_held: false,
            left_held: false,
            right_held: false,
            enemy_depth: 0.0,
            enemy_x: 0.2,
            hits: 0,
        }
    }

    fn update(&mut self) {
        let speed = 0.02;
        if self.up_held {
            self.reticle_y -= speed;
        }
        if self.down_held {
            self.reticle_y += speed;
        }
        if self.left_held {
            self.reticle_x -= speed;
        }
        if self.right_held {
            self.reticle_x += speed;
        }
        self.reticle_x = self.reticle_x.clamp(-1.0, 1.0);
        self.reticle_y = self.reticle_y.clamp(-1.0, 0.5);

        self.enemy_depth += 0.006;
        if self.enemy_depth >= 1.0 {
            // 目前まで来て未撃墜ならリセット(ミス扱い、減点は今回未実装)。
            self.enemy_depth = 0.0;
            self.enemy_x = if self.enemy_x > 0.0 { -0.3 } else { 0.3 };
            return;
        }

        // ヒット判定: 敵が十分近く(depth>0.7)、照準が敵の位置に十分近ければ命中。
        let enemy_half = self.enemy_half_size();
        let dx = (self.reticle_x - self.enemy_x).abs();
        let dy = (self.reticle_y - self.enemy_y()).abs();
        if self.enemy_depth > 0.7 && dx < enemy_half * 1.5 && dy < enemy_half * 1.5 {
            self.hits += 1;
            println!("HIT! total={}", self.hits);
            self.enemy_depth = 0.0;
            self.enemy_x = if self.enemy_x > 0.0 { -0.3 } else { 0.3 };
        }
    }

    fn enemy_half_size(&self) -> f32 {
        0.03 + self.enemy_depth * 0.35
    }

    fn enemy_y(&self) -> f32 {
        // 奥(小さい)ほど上寄り、近い(大きい)ほど下寄りに見せる疑似パース。
        -0.4 + self.enemy_depth * 0.5
    }

    fn reticle_dest_ndc(&self) -> [f32; 4] {
        let h = 0.04;
        [self.reticle_x - h, self.reticle_y - h, self.reticle_x + h, self.reticle_y + h]
    }

    fn enemy_dest_ndc(&self) -> [f32; 4] {
        let h = self.enemy_half_size();
        let ey = self.enemy_y();
        [self.enemy_x - h, ey - h, self.enemy_x + h, ey + h]
    }

    fn ground_dest_ndc(&self) -> [f32; 4] {
        [-1.0, 0.5, 1.0, 1.0]
    }
}

fn main() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("sample-harrier: pseudo-3D scaling sprite prototype (Phase 0)")
        .with_inner_size(winit::dpi::LogicalSize::new(WIDTH, HEIGHT))
        .build(&event_loop)
        .expect("create window");

    let mut renderer = unsafe { Renderer::new(&window) }.expect("create Vulkan renderer");
    let mut state = GameState::new();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent { event: WindowEvent::KeyboardInput { input, .. }, .. } => {
                let pressed = input.state == ElementState::Pressed;
                match input.virtual_keycode {
                    Some(VirtualKeyCode::Left) | Some(VirtualKeyCode::A) => state.left_held = pressed,
                    Some(VirtualKeyCode::Right) | Some(VirtualKeyCode::D) => state.right_held = pressed,
                    Some(VirtualKeyCode::Up) | Some(VirtualKeyCode::W) => state.up_held = pressed,
                    Some(VirtualKeyCode::Down) | Some(VirtualKeyCode::S) => state.down_held = pressed,
                    Some(VirtualKeyCode::Escape) => *control_flow = ControlFlow::Exit,
                    _ => {}
                }
            }
            Event::MainEventsCleared => {
                state.update();
                unsafe { renderer.draw_frame(&state) };
            }
            _ => {}
        }
    });
}

/// 実ウィンドウ向けVulkanレンダラー。`open-directx`の
/// `directx-graphics-window`と同一パターン(固定サイズスワップチェーン、
/// 単一コマンドバッファ+フェンス待機)。リサイズ非対応。
struct Renderer {
    _entry: Entry,
    instance: ash::Instance,
    surface_loader: ash::extensions::khr::Surface,
    surface: vk::SurfaceKHR,
    device: ash::Device,
    queue: vk::Queue,
    swapchain_loader: ash::extensions::khr::Swapchain,
    swapchain: vk::SwapchainKHR,
    _swapchain_format: vk::Format,
    swapchain_extent: vk::Extent2D,
    swapchain_image_views: Vec<vk::ImageView>,
    render_pass: vk::RenderPass,
    framebuffers: Vec<vk::Framebuffer>,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set_layout: vk::DescriptorSetLayout,
    ground_descriptor_set: vk::DescriptorSet,
    enemy_descriptor_set: vk::DescriptorSet,
    reticle_descriptor_set: vk::DescriptorSet,
    vertex_buffer: vk::Buffer,
    vertex_buffer_memory: vk::DeviceMemory,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    image_available: vk::Semaphore,
    render_finished: vk::Semaphore,
    in_flight: vk::Fence,
    _ground_texture_image: vk::Image,
    _enemy_texture_image: vk::Image,
    _reticle_texture_image: vk::Image,
    sampler: vk::Sampler,
}

impl Renderer {
    unsafe fn new(window: &winit::window::Window) -> Result<Self, String> {
        let entry = Entry::load().map_err(|e| e.to_string())?;

        let app_name = CString::new("sample-harrier").unwrap();
        let app_info = vk::ApplicationInfo::builder().application_name(&app_name).api_version(vk::API_VERSION_1_1);
        let display_handle = window.raw_display_handle();
        let window_handle = window.raw_window_handle();
        let required_extensions =
            ash_window::enumerate_required_extensions(display_handle).map_err(|e| e.to_string())?;
        let instance_info = vk::InstanceCreateInfo::builder().application_info(&app_info).enabled_extension_names(required_extensions);
        let instance = entry.create_instance(&instance_info, None).map_err(|e| e.to_string())?;

        let surface = ash_window::create_surface(&entry, &instance, display_handle, window_handle, None).map_err(|e| e.to_string())?;
        let surface_loader = ash::extensions::khr::Surface::new(&entry, &instance);

        let physical_devices = instance.enumerate_physical_devices().map_err(|e| e.to_string())?;
        let mut selected: Option<(vk::PhysicalDevice, u32)> = None;
        for &pd in &physical_devices {
            let families = instance.get_physical_device_queue_family_properties(pd);
            for (idx, f) in families.iter().enumerate() {
                let supports_present = surface_loader.get_physical_device_surface_support(pd, idx as u32, surface).unwrap_or(false);
                if f.queue_flags.contains(vk::QueueFlags::GRAPHICS) && supports_present {
                    selected = Some((pd, idx as u32));
                    break;
                }
            }
            if selected.is_some() {
                break;
            }
        }
        let (physical_device, queue_family_index) = selected.ok_or("no graphics+present capable device found")?;

        let priorities = [1.0f32];
        let queue_info = [vk::DeviceQueueCreateInfo::builder().queue_family_index(queue_family_index).queue_priorities(&priorities).build()];
        let device_extensions = [ash::extensions::khr::Swapchain::name().as_ptr()];
        let device_info = vk::DeviceCreateInfo::builder().queue_create_infos(&queue_info).enabled_extension_names(&device_extensions);
        let device = instance.create_device(physical_device, &device_info, None).map_err(|e| e.to_string())?;
        let queue = device.get_device_queue(queue_family_index, 0);
        let memory_properties = instance.get_physical_device_memory_properties(physical_device);

        let swapchain_loader = ash::extensions::khr::Swapchain::new(&instance, &device);
        let surface_caps = surface_loader.get_physical_device_surface_capabilities(physical_device, surface).map_err(|e| e.to_string())?;
        let surface_formats = surface_loader.get_physical_device_surface_formats(physical_device, surface).map_err(|e| e.to_string())?;
        let surface_format = surface_formats
            .iter()
            .find(|f| f.format == vk::Format::B8G8R8A8_UNORM)
            .copied()
            .unwrap_or(surface_formats[0]);
        let present_mode = vk::PresentModeKHR::FIFO;
        let extent = vk::Extent2D { width: WIDTH, height: HEIGHT };
        let image_count = (surface_caps.min_image_count + 1).min(if surface_caps.max_image_count > 0 { surface_caps.max_image_count } else { u32::MAX });

        let swapchain_info = vk::SwapchainCreateInfoKHR::builder()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(surface_caps.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true);
        let swapchain = swapchain_loader.create_swapchain(&swapchain_info, None).map_err(|e| e.to_string())?;
        let swapchain_images = swapchain_loader.get_swapchain_images(swapchain).map_err(|e| e.to_string())?;

        let swapchain_image_views: Vec<vk::ImageView> = swapchain_images
            .iter()
            .map(|&img| {
                let info = vk::ImageViewCreateInfo::builder()
                    .image(img)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(surface_format.format)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    });
                device.create_image_view(&info, None).expect("create swapchain image view")
            })
            .collect();

        let attachment = vk::AttachmentDescription::builder()
            .format(surface_format.format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)
            .build();
        let color_ref = vk::AttachmentReference { attachment: 0, layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL };
        let color_refs = [color_ref];
        let subpass = vk::SubpassDescription::builder().pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS).color_attachments(&color_refs).build();
        let attachments = [attachment];
        let subpasses = [subpass];
        let dependency = vk::SubpassDependency::builder()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .build();
        let dependencies = [dependency];
        let render_pass_info = vk::RenderPassCreateInfo::builder().attachments(&attachments).subpasses(&subpasses).dependencies(&dependencies);
        let render_pass = device.create_render_pass(&render_pass_info, None).map_err(|e| e.to_string())?;

        let framebuffers: Vec<vk::Framebuffer> = swapchain_image_views
            .iter()
            .map(|&view| {
                let attachments_fb = [view];
                let info = vk::FramebufferCreateInfo::builder()
                    .render_pass(render_pass)
                    .attachments(&attachments_fb)
                    .width(extent.width)
                    .height(extent.height)
                    .layers(1);
                device.create_framebuffer(&info, None).expect("create framebuffer")
            })
            .collect();

        let (ground_image, ground_view) = create_solid_texture(&device, &memory_properties, queue, queue_family_index, Rgba8 { r: 30, g: 140, b: 40, a: 255 })?;
        let (enemy_image, enemy_view) = create_solid_texture(&device, &memory_properties, queue, queue_family_index, Rgba8 { r: 210, g: 30, b: 30, a: 255 })?;
        let (reticle_image, reticle_view) = create_solid_texture(&device, &memory_properties, queue, queue_family_index, Rgba8 { r: 240, g: 220, b: 30, a: 255 })?;

        let sampler_info = vk::SamplerCreateInfo::builder().mag_filter(vk::Filter::NEAREST).min_filter(vk::Filter::NEAREST);
        let sampler = device.create_sampler(&sampler_info, None).map_err(|e| e.to_string())?;

        let dsl_binding = vk::DescriptorSetLayoutBinding::builder()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            .build();
        let dsl_bindings = [dsl_binding];
        let dsl_info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&dsl_bindings);
        let descriptor_set_layout = device.create_descriptor_set_layout(&dsl_info, None).map_err(|e| e.to_string())?;

        let pool_size = vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: 3 };
        let pool_sizes = [pool_size];
        let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder().pool_sizes(&pool_sizes).max_sets(3);
        let descriptor_pool = device.create_descriptor_pool(&descriptor_pool_info, None).map_err(|e| e.to_string())?;

        let set_layouts = [descriptor_set_layout, descriptor_set_layout, descriptor_set_layout];
        let ds_alloc_info = vk::DescriptorSetAllocateInfo::builder().descriptor_pool(descriptor_pool).set_layouts(&set_layouts);
        let sets = device.allocate_descriptor_sets(&ds_alloc_info).map_err(|e| e.to_string())?;
        let (ground_descriptor_set, enemy_descriptor_set, reticle_descriptor_set) = (sets[0], sets[1], sets[2]);

        for (set, view) in [(ground_descriptor_set, ground_view), (enemy_descriptor_set, enemy_view), (reticle_descriptor_set, reticle_view)] {
            let image_info = vk::DescriptorImageInfo::builder()
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_view(view)
                .sampler(sampler)
                .build();
            let image_infos = [image_info];
            let write = vk::WriteDescriptorSet::builder()
                .dst_set(set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&image_infos)
                .build();
            device.update_descriptor_sets(&[write], &[]);
        }

        let vbuf_size = (18 * std::mem::size_of::<Vertex>()) as vk::DeviceSize;
        let (vertex_buffer, vertex_buffer_memory) =
            create_host_visible_buffer(&device, &memory_properties, vbuf_size, vk::BufferUsageFlags::VERTEX_BUFFER)?;

        let vs = translate_sprite_vertex_shader(include_bytes!("../../open-directx/crates/directx-shader-translate/shaders/sprite_vs.dxbc")).map_err(|e| e.to_string())?;
        let ps = translate_sprite_pixel_shader(include_bytes!("../../open-directx/crates/directx-shader-translate/shaders/sprite_ps.dxbc")).map_err(|e| e.to_string())?;

        let vs_module_info = vk::ShaderModuleCreateInfo::builder().code(&vs.spirv_words);
        let vs_module = device.create_shader_module(&vs_module_info, None).map_err(|e| e.to_string())?;
        let ps_module_info = vk::ShaderModuleCreateInfo::builder().code(&ps.spirv_words);
        let ps_module = device.create_shader_module(&ps_module_info, None).map_err(|e| e.to_string())?;

        let entry_name = CString::new("main").unwrap();
        let stages = [
            vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::VERTEX).module(vs_module).name(&entry_name).build(),
            vk::PipelineShaderStageCreateInfo::builder().stage(vk::ShaderStageFlags::FRAGMENT).module(ps_module).name(&entry_name).build(),
        ];

        let binding_desc = vk::VertexInputBindingDescription { binding: 0, stride: std::mem::size_of::<Vertex>() as u32, input_rate: vk::VertexInputRate::VERTEX };
        let attr_descs = [
            vk::VertexInputAttributeDescription { location: 0, binding: 0, format: vk::Format::R32G32B32_SFLOAT, offset: 0 },
            vk::VertexInputAttributeDescription { location: 1, binding: 0, format: vk::Format::R32G32_SFLOAT, offset: std::mem::size_of::<[f32; 3]>() as u32 },
        ];
        let binding_descs = [binding_desc];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::builder().vertex_binding_descriptions(&binding_descs).vertex_attribute_descriptions(&attr_descs);
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::builder().topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::Viewport { x: 0.0, y: 0.0, width: extent.width as f32, height: extent.height as f32, min_depth: 0.0, max_depth: 1.0 };
        let scissor = vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent };
        let viewports = [viewport];
        let scissors = [scissor];
        let viewport_state = vk::PipelineViewportStateCreateInfo::builder().viewports(&viewports).scissors(&scissors);
        let rasterization = vk::PipelineRasterizationStateCreateInfo::builder()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::builder().rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let color_blend_attachment = vk::PipelineColorBlendAttachmentState { blend_enable: vk::FALSE, color_write_mask: vk::ColorComponentFlags::RGBA, ..Default::default() };
        let color_blend_attachments = [color_blend_attachment];
        let color_blend = vk::PipelineColorBlendStateCreateInfo::builder().attachments(&color_blend_attachments);
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(&set_layouts[..1]);
        let pipeline_layout = device.create_pipeline_layout(&pipeline_layout_info, None).map_err(|e| e.to_string())?;
        let pipeline_info = vk::GraphicsPipelineCreateInfo::builder()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterization)
            .multisample_state(&multisample)
            .color_blend_state(&color_blend)
            .layout(pipeline_layout)
            .render_pass(render_pass)
            .subpass(0)
            .build();
        let pipeline = device
            .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .map_err(|(_, e)| e.to_string())?[0];
        device.destroy_shader_module(vs_module, None);
        device.destroy_shader_module(ps_module, None);

        let pool_info = vk::CommandPoolCreateInfo::builder().queue_family_index(queue_family_index).flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = device.create_command_pool(&pool_info, None).map_err(|e| e.to_string())?;
        let cmd_alloc_info = vk::CommandBufferAllocateInfo::builder().command_pool(command_pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
        let command_buffer = device.allocate_command_buffers(&cmd_alloc_info).map_err(|e| e.to_string())?[0];

        let sem_info = vk::SemaphoreCreateInfo::builder();
        let image_available = device.create_semaphore(&sem_info, None).map_err(|e| e.to_string())?;
        let render_finished = device.create_semaphore(&sem_info, None).map_err(|e| e.to_string())?;
        let fence_info = vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED);
        let in_flight = device.create_fence(&fence_info, None).map_err(|e| e.to_string())?;

        println!(
            "sample-harrier: 実ウィンドウ+実スワップチェーンを作成しました({}x{}, format_raw={}, images={})",
            extent.width,
            extent.height,
            surface_format.format.as_raw(),
            swapchain_images.len()
        );

        Ok(Renderer {
            _entry: entry,
            instance,
            surface_loader,
            surface,
            device,
            queue,
            swapchain_loader,
            swapchain,
            _swapchain_format: surface_format.format,
            swapchain_extent: extent,
            swapchain_image_views,
            render_pass,
            framebuffers,
            pipeline_layout,
            pipeline,
            descriptor_pool,
            descriptor_set_layout,
            ground_descriptor_set,
            enemy_descriptor_set,
            reticle_descriptor_set,
            vertex_buffer,
            vertex_buffer_memory,
            command_pool,
            command_buffer,
            image_available,
            render_finished,
            in_flight,
            _ground_texture_image: ground_image,
            _enemy_texture_image: enemy_image,
            _reticle_texture_image: reticle_image,
            sampler,
        })
    }

    unsafe fn draw_frame(&mut self, state: &GameState) {
        self.device.wait_for_fences(&[self.in_flight], true, u64::MAX).expect("wait_for_fences");

        let (image_index, _) = match self.swapchain_loader.acquire_next_image(self.swapchain, u64::MAX, self.image_available, vk::Fence::null()) {
            Ok(r) => r,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return,
            Err(e) => panic!("vkAcquireNextImageKHR failed: {e}"),
        };
        self.device.reset_fences(&[self.in_flight]).expect("reset_fences");

        let ground_verts = quad_vertices(state.ground_dest_ndc());
        let enemy_verts = quad_vertices(state.enemy_dest_ndc());
        let reticle_verts = quad_vertices(state.reticle_dest_ndc());
        let mut all_verts = [Vertex { pos: [0.0; 3], uv: [0.0; 2] }; 18];
        all_verts[0..6].copy_from_slice(&ground_verts);
        all_verts[6..12].copy_from_slice(&enemy_verts);
        all_verts[12..18].copy_from_slice(&reticle_verts);
        let vbuf_size = std::mem::size_of_val(&all_verts) as vk::DeviceSize;
        let ptr = self.device.map_memory(self.vertex_buffer_memory, 0, vbuf_size, vk::MemoryMapFlags::empty()).expect("map vertex buffer");
        std::ptr::copy_nonoverlapping(all_verts.as_ptr() as *const u8, ptr as *mut u8, vbuf_size as usize);
        self.device.unmap_memory(self.vertex_buffer_memory);

        self.device.reset_command_buffer(self.command_buffer, vk::CommandBufferResetFlags::empty()).expect("reset command buffer");
        let begin_info = vk::CommandBufferBeginInfo::builder();
        self.device.begin_command_buffer(self.command_buffer, &begin_info).expect("begin command buffer");

        let clear_value = vk::ClearValue { color: vk::ClearColorValue { float32: [0.4, 0.7, 0.95, 1.0] } };
        let clear_values = [clear_value];
        let render_pass_begin = vk::RenderPassBeginInfo::builder()
            .render_pass(self.render_pass)
            .framebuffer(self.framebuffers[image_index as usize])
            .render_area(vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent: self.swapchain_extent })
            .clear_values(&clear_values);
        self.device.cmd_begin_render_pass(self.command_buffer, &render_pass_begin, vk::SubpassContents::INLINE);
        self.device.cmd_bind_pipeline(self.command_buffer, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
        self.device.cmd_bind_vertex_buffers(self.command_buffer, 0, &[self.vertex_buffer], &[0]);

        for (set, first_vertex) in [(self.ground_descriptor_set, 0), (self.enemy_descriptor_set, 6), (self.reticle_descriptor_set, 12)] {
            self.device.cmd_bind_descriptor_sets(self.command_buffer, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 0, &[set], &[]);
            self.device.cmd_draw(self.command_buffer, 6, 1, first_vertex, 0);
        }

        self.device.cmd_end_render_pass(self.command_buffer);
        self.device.end_command_buffer(self.command_buffer).expect("end command buffer");

        let wait_semaphores = [self.image_available];
        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let signal_semaphores = [self.render_finished];
        let command_buffers = [self.command_buffer];
        let submit_info = vk::SubmitInfo::builder()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&command_buffers)
            .signal_semaphores(&signal_semaphores)
            .build();
        self.device.queue_submit(self.queue, &[submit_info], self.in_flight).expect("queue_submit");

        let swapchains = [self.swapchain];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR::builder().wait_semaphores(&signal_semaphores).swapchains(&swapchains).image_indices(&image_indices);
        match self.swapchain_loader.queue_present(self.queue, &present_info) {
            Ok(_) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {}
            Err(e) => panic!("vkQueuePresentKHR failed: {e}"),
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_semaphore(self.image_available, None);
            self.device.destroy_semaphore(self.render_finished, None);
            self.device.destroy_fence(self.in_flight, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.destroy_descriptor_pool(self.descriptor_pool, None);
            self.device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            self.device.destroy_sampler(self.sampler, None);
            self.device.destroy_buffer(self.vertex_buffer, None);
            self.device.free_memory(self.vertex_buffer_memory, None);
            for &fb in &self.framebuffers {
                self.device.destroy_framebuffer(fb, None);
            }
            self.device.destroy_render_pass(self.render_pass, None);
            for &view in &self.swapchain_image_views {
                self.device.destroy_image_view(view, None);
            }
            self.swapchain_loader.destroy_swapchain(self.swapchain, None);
            self.device.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            self.instance.destroy_instance(None);
        }
    }
}

unsafe fn create_host_visible_buffer(
    device: &ash::Device,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory), String> {
    let buffer_info = vk::BufferCreateInfo::builder().size(size).usage(usage).sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = device.create_buffer(&buffer_info, None).map_err(|e| e.to_string())?;
    let mem_req = device.get_buffer_memory_requirements(buffer);
    let mem_type = find_memory_type(memory_properties, mem_req.memory_type_bits, vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)
        .ok_or("no suitable host-visible memory type")?;
    let alloc_info = vk::MemoryAllocateInfo::builder().allocation_size(mem_req.size).memory_type_index(mem_type);
    let memory = device.allocate_memory(&alloc_info, None).map_err(|e| e.to_string())?;
    device.bind_buffer_memory(buffer, memory, 0).map_err(|e| e.to_string())?;
    Ok((buffer, memory))
}

fn find_memory_type(props: &vk::PhysicalDeviceMemoryProperties, type_bits: u32, required: vk::MemoryPropertyFlags) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| type_bits & (1 << i) != 0 && props.memory_types[i as usize].property_flags.contains(required))
}

unsafe fn create_solid_texture(
    device: &ash::Device,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    queue_family_index: u32,
    color: Rgba8,
) -> Result<(vk::Image, vk::ImageView), String> {
    let texture = TextureRgba8 { width: 1, height: 1, pixels: vec![color] };
    const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

    let tex_bytes_size = 4u64;
    let (staging_buffer, staging_memory) = create_host_visible_buffer(device, memory_properties, tex_bytes_size, vk::BufferUsageFlags::TRANSFER_SRC)?;
    let ptr = device.map_memory(staging_memory, 0, tex_bytes_size, vk::MemoryMapFlags::empty()).map_err(|e| e.to_string())?;
    let dst = std::slice::from_raw_parts_mut(ptr as *mut u8, 4);
    let p = texture.pixels[0];
    dst.copy_from_slice(&[p.r, p.g, p.b, p.a]);
    device.unmap_memory(staging_memory);

    let image_info = vk::ImageCreateInfo::builder()
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D { width: 1, height: 1, depth: 1 })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    let image = device.create_image(&image_info, None).map_err(|e| e.to_string())?;
    let mem_req = device.get_image_memory_requirements(image);
    let mem_type = find_memory_type(memory_properties, mem_req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL).ok_or("no device-local memory type")?;
    let alloc_info = vk::MemoryAllocateInfo::builder().allocation_size(mem_req.size).memory_type_index(mem_type);
    let memory = device.allocate_memory(&alloc_info, None).map_err(|e| e.to_string())?;
    device.bind_image_memory(image, memory, 0).map_err(|e| e.to_string())?;

    let view_info = vk::ImageViewCreateInfo::builder()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(FORMAT)
        .subresource_range(vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 });
    let view = device.create_image_view(&view_info, None).map_err(|e| e.to_string())?;

    let pool_info = vk::CommandPoolCreateInfo::builder().queue_family_index(queue_family_index);
    let pool = device.create_command_pool(&pool_info, None).map_err(|e| e.to_string())?;
    let cmd_alloc_info = vk::CommandBufferAllocateInfo::builder().command_pool(pool).level(vk::CommandBufferLevel::PRIMARY).command_buffer_count(1);
    let cmd = device.allocate_command_buffers(&cmd_alloc_info).map_err(|e| e.to_string())?[0];
    let begin_info = vk::CommandBufferBeginInfo::builder();
    device.begin_command_buffer(cmd, &begin_info).map_err(|e| e.to_string())?;

    let subresource = vk::ImageSubresourceRange { aspect_mask: vk::ImageAspectFlags::COLOR, base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1 };
    let to_dst = vk::ImageMemoryBarrier::builder()
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .image(image)
        .subresource_range(subresource)
        .build();
    device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TOP_OF_PIPE, vk::PipelineStageFlags::TRANSFER, vk::DependencyFlags::empty(), &[], &[], &[to_dst]);
    let copy_region = vk::BufferImageCopy {
        buffer_offset: 0,
        buffer_row_length: 0,
        buffer_image_height: 0,
        image_subresource: vk::ImageSubresourceLayers { aspect_mask: vk::ImageAspectFlags::COLOR, mip_level: 0, base_array_layer: 0, layer_count: 1 },
        image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
        image_extent: vk::Extent3D { width: 1, height: 1, depth: 1 },
    };
    device.cmd_copy_buffer_to_image(cmd, staging_buffer, image, vk::ImageLayout::TRANSFER_DST_OPTIMAL, &[copy_region]);
    let to_read = vk::ImageMemoryBarrier::builder()
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .image(image)
        .subresource_range(subresource)
        .build();
    device.cmd_pipeline_barrier(cmd, vk::PipelineStageFlags::TRANSFER, vk::PipelineStageFlags::FRAGMENT_SHADER, vk::DependencyFlags::empty(), &[], &[], &[to_read]);
    device.end_command_buffer(cmd).map_err(|e| e.to_string())?;

    let fence = device.create_fence(&vk::FenceCreateInfo::builder(), None).map_err(|e| e.to_string())?;
    let cmds = [cmd];
    let submit = vk::SubmitInfo::builder().command_buffers(&cmds).build();
    device.queue_submit(queue, &[submit], fence).map_err(|e| e.to_string())?;
    device.wait_for_fences(&[fence], true, u64::MAX).map_err(|e| e.to_string())?;

    device.destroy_fence(fence, None);
    device.destroy_command_pool(pool, None);
    device.destroy_buffer(staging_buffer, None);
    device.free_memory(staging_memory, None);

    Ok((image, view))
}
