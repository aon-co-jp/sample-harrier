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
//! 弾+爆発ギミック(ユーザー指示、2026-08-10「弾を打てて当たると爆発する
//! と良い」への対応): Spaceキーで照準の位置へ向けて弾(白)を発射する
//! (最大`MAX_BULLETS`発まで同時飛行、`FIRE_COOLDOWN_FRAMES`フレームの
//! クールダウン)。弾は画面下(自機付近)から照準位置へ`BULLET_TRAVEL_FRAMES`
//! フレームで直進し、着弾時に敵と十分近ければ命中——命中位置に爆発
//! エフェクト(橙、`EXPLOSION_LIFETIME_FRAMES`フレームで拡大→消滅)を
//! 発生させ、敵を奥へリセットする。外れた場合は爆発無しでそのまま弾が
//! 消える。
//!
//! 追加ギミック(ユーザー指示、2026-08-10「弾は近くから遠くに飛んでいく
//! 大きな弾が縮小していく感じ、ハリアーのキャラクターと敵とボスを作って
//! ハリアー風パロディBGMにして、床をラスタースクロールして」への対応):
//! - 弾は発射直後(自機付近=近距離)が大きく、飛んでいくにつれて
//!   (=画面奥へ向かうにつれて)縮小する——スペースハリアーの疑似3D
//!   スケーリングの向きを弾にも適用した。
//! - 自機(青いハリアー機のプレースホルダー)を照準の下に描画。
//! - `BOSS_EVERY_N_DEFEATS`体撃墜するごとにボス(大型・複数回被弾が
//!   必要)が出現する。
//! - 床は単色の帯ではなく、`FLOOR_STRIPES`本の横帯が交互の色で
//!   奥から手前へ流れる(疑似ラスタースクロール)。
//! - BGMは既存楽曲の再現ではなく、8bit・疾走感のある曲想のみを参考に
//!   した完全新規オリジナル曲(`audio.rs`)をループ再生する。
//!
//! 実行方法: `cargo run --release`

mod audio;
mod font;
mod sprite;

use std::ffi::CString;

use ash::{vk, Entry};
use directx_graphics_vulkan::{Rgba8, TextureRgba8};
use directx_shader_translate::spirv_gen::{translate_sprite_pixel_shader, translate_sprite_vertex_shader};
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};
use winit::event::{ElementState, Event, VirtualKeyCode, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

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

const MAX_BULLETS: usize = 3;
const MAX_EXPLOSIONS: usize = 3;
const FIRE_COOLDOWN_FRAMES: u32 = 12;
const BULLET_TRAVEL_FRAMES: f32 = 16.0;
const EXPLOSION_LIFETIME_FRAMES: u32 = 18;

/// この数だけ通常の敵を倒すごとに、次の1体はボスになる。
const BOSS_EVERY_N_DEFEATS: u32 = 3;
/// ボスを倒すのに必要な命中回数(通常の敵は1回)。
const BOSS_HITS_REQUIRED: u32 = 3;
const FLOOR_STRIPES: usize = 8;
const FLOOR_SCROLL_SPEED: f32 = 0.01;

#[derive(Clone, Copy)]
struct Bullet {
    active: bool,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    t: f32,
}

impl Bullet {
    const INACTIVE: Bullet = Bullet { active: false, x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0, t: 0.0 };

    fn pos(&self) -> (f32, f32) {
        (self.x0 + (self.x1 - self.x0) * self.t, self.y0 + (self.y1 - self.y0) * self.t)
    }
}

#[derive(Clone, Copy)]
struct Explosion {
    active: bool,
    x: f32,
    y: f32,
    timer: u32,
}

impl Explosion {
    const INACTIVE: Explosion = Explosion { active: false, x: 0.0, y: 0.0, timer: 0 };
}

/// ゲーム状態: 照準(自機)+奥から迫る敵1体+固定の地面帯+弾+爆発。
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
    defeats: u32,
    is_boss: bool,
    boss_hits_remaining: u32,
    fire_held: bool,
    fire_cooldown: u32,
    bullets: [Bullet; MAX_BULLETS],
    explosions: [Explosion; MAX_EXPLOSIONS],
    floor_scroll: f32,
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
            defeats: 0,
            is_boss: false,
            boss_hits_remaining: BOSS_HITS_REQUIRED,
            fire_held: false,
            fire_cooldown: 0,
            bullets: [Bullet::INACTIVE; MAX_BULLETS],
            explosions: [Explosion::INACTIVE; MAX_EXPLOSIONS],
            floor_scroll: 0.0,
        }
    }

    fn spawn_next_enemy(&mut self, from_positive_x: bool) {
        self.enemy_depth = 0.0;
        self.enemy_x = if from_positive_x { -0.3 } else { 0.3 };
        self.is_boss = self.defeats > 0 && self.defeats % BOSS_EVERY_N_DEFEATS == 0;
        self.boss_hits_remaining = BOSS_HITS_REQUIRED;
        if self.is_boss {
            println!("ボス出現! (被弾{}回で撃破)", BOSS_HITS_REQUIRED);
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

        self.floor_scroll = (self.floor_scroll + FLOOR_SCROLL_SPEED) % 1.0;

        self.enemy_depth += 0.006;
        let mut enemy_hit_this_frame = false;
        if self.enemy_depth >= 1.0 {
            // 目前まで来て未撃墜ならリセット(ミス扱い、減点は今回未実装)。
            let from_positive = self.enemy_x > 0.0;
            self.spawn_next_enemy(from_positive);
        } else {
            // ヒット判定: 敵が十分近く(depth>0.7)、照準が敵の位置に十分近ければ命中
            // (弾を撃たなくても近距離で自然に倒せる、既存のパッシブ判定)。
            let enemy_half = self.enemy_half_size();
            let dx = (self.reticle_x - self.enemy_x).abs();
            let dy = (self.reticle_y - self.enemy_y()).abs();
            if self.enemy_depth > 0.7 && dx < enemy_half * 1.5 && dy < enemy_half * 1.5 {
                enemy_hit_this_frame = true;
            }
        }

        // 発射(Spaceキー)。クールダウン中・空きスロットが無ければ無視。
        if self.fire_held && self.fire_cooldown == 0 {
            if let Some(slot) = self.bullets.iter_mut().find(|b| !b.active) {
                *slot = Bullet { active: true, x0: 0.0, y0: 0.9, x1: self.reticle_x, y1: self.reticle_y, t: 0.0 };
                self.fire_cooldown = FIRE_COOLDOWN_FRAMES;
            }
        }
        self.fire_cooldown = self.fire_cooldown.saturating_sub(1);

        // 弾の飛行と着弾判定。
        let enemy_half = self.enemy_half_size();
        let enemy_x = self.enemy_x;
        let enemy_y = self.enemy_y();
        let enemy_depth = self.enemy_depth;
        let mut new_explosion: Option<(f32, f32)> = None;
        for bullet in self.bullets.iter_mut() {
            if !bullet.active {
                continue;
            }
            bullet.t += 1.0 / BULLET_TRAVEL_FRAMES;
            if bullet.t >= 1.0 {
                bullet.active = false;
                let dx = (bullet.x1 - enemy_x).abs();
                let dy = (bullet.y1 - enemy_y).abs();
                if enemy_depth > 0.05 && dx < enemy_half * 1.3 && dy < enemy_half * 1.3 {
                    enemy_hit_this_frame = true;
                    new_explosion = Some((bullet.x1, bullet.y1));
                }
            }
        }
        if let Some((x, y)) = new_explosion {
            if let Some(exp) = self.explosions.iter_mut().find(|e| !e.active) {
                *exp = Explosion { active: true, x, y, timer: EXPLOSION_LIFETIME_FRAMES };
            }
        }

        if enemy_hit_this_frame {
            self.hits += 1;
            if self.is_boss && self.boss_hits_remaining > 1 {
                // ボスは複数回被弾するまで倒れない——ノックバックのみ
                // (奥へ少し戻す)、まだ撃墜扱いにはしない。
                self.boss_hits_remaining -= 1;
                self.enemy_depth = (self.enemy_depth - 0.3).max(0.0);
                println!("HIT! ボスに命中(残り{}発)", self.boss_hits_remaining);
            } else {
                println!("HIT! total={}", self.hits);
                let from_positive = self.enemy_x > 0.0;
                if self.is_boss {
                    println!("ボス撃破!");
                }
                self.defeats += 1;
                self.spawn_next_enemy(from_positive);
            }
        }

        for explosion in self.explosions.iter_mut() {
            if explosion.active {
                explosion.timer = explosion.timer.saturating_sub(1);
                if explosion.timer == 0 {
                    explosion.active = false;
                }
            }
        }
    }

    fn enemy_half_size(&self) -> f32 {
        let base = 0.03 + self.enemy_depth * 0.35;
        if self.is_boss { base * 1.8 } else { base }
    }

    fn enemy_y(&self) -> f32 {
        // 奥(小さい)ほど上寄り、近い(大きい)ほど下寄りに見せる疑似パース。
        -0.4 + self.enemy_depth * 0.5
    }

    fn reticle_dest_ndc(&self) -> [f32; 4] {
        let h = 0.04;
        [self.reticle_x - h, self.reticle_y - h, self.reticle_x + h, self.reticle_y + h]
    }

    /// 自機(ハリアー機のプレースホルダー)、照準のすぐ下に表示する。
    fn ship_dest_ndc(&self) -> [f32; 4] {
        let cx = self.reticle_x;
        let cy = (self.reticle_y + 0.18).min(0.6);
        [cx - 0.07, cy - 0.03, cx + 0.07, cy + 0.03]
    }

    fn enemy_dest_ndc(&self) -> [f32; 4] {
        let h = self.enemy_half_size();
        let ey = self.enemy_y();
        [self.enemy_x - h, ey - h, self.enemy_x + h, ey + h]
    }

    /// 疑似ラスタースクロールする床。`FLOOR_STRIPES`本の横帯を、
    /// 遠い(上側)ほど薄く・近い(下側)ほど厚く描き、`floor_scroll`に
    /// 応じて奥から手前へ流れるように各帯のY位置をずらす。
    fn floor_stripe_dest_ndc(&self, i: usize) -> [f32; 4] {
        let n = FLOOR_STRIPES as f32;
        // 遠近感: 遠い帯ほど薄く、近い帯ほど厚くなる等比的な配分。
        let t0 = (i as f32 + self.floor_scroll) / n;
        let t1 = (i as f32 + 1.0 + self.floor_scroll) / n;
        let y0 = 0.5 + 0.5 * t0 * t0;
        let y1 = 0.5 + 0.5 * t1 * t1;
        [-1.0, y0.min(1.0), 1.0, y1.min(1.0)]
    }

    fn bullet_dest_ndc(&self, i: usize) -> [f32; 4] {
        let b = &self.bullets[i];
        if !b.active {
            return [-2.0, -2.0, -1.9, -1.9];
        }
        let (x, y) = b.pos();
        // 発射直後(自機付近、近距離)は大きく、飛んでいくにつれて
        // (=奥へ向かうにつれて)縮小する——スケーリングスプライトの
        // 向きを弾にも適用した(ユーザー指示、2026-08-10)。
        let h = 0.05 * (1.0 - b.t) + 0.01 * b.t;
        [x - h, y - h, x + h, y + h]
    }

    fn explosion_dest_ndc(&self, i: usize) -> [f32; 4] {
        let e = &self.explosions[i];
        if !e.active {
            return [-2.0, -2.0, -1.9, -1.9];
        }
        // 発生直後に急拡大し、その後じわじわ小さくなりながら消える。
        let progress = 1.0 - (e.timer as f32 / EXPLOSION_LIFETIME_FRAMES as f32);
        let h = 0.02 + 0.12 * (1.0 - (progress - 0.3).abs().min(1.0));
        [e.x - h, e.y - h, e.x + h, e.y + h]
    }
}

fn main() {
    // 日本語の操作説明はコンソールへ出力する(ユーザー指示、2026-08-10
    // 「操作説明は下側に付けて英語と日本語で」——英文は画面下のHUDバナー
    // (`font.rs`の自作5x7ビットマップフォント)で常時表示し、日本語グリフ
    // は用意していないためコンソール/この起動メッセージ側で対応する)。
    println!("=== sample-harrier 操作方法 ===");
    println!("矢印キー/WASD: 照準移動   Space: 発射   Esc または ウィンドウのXボタン: 終了");

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("sample-harrier: pseudo-3D scaling sprite prototype (Phase 0)")
        .with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)))
        .build(&event_loop)
        .expect("create window");

    // BGM再生ハンドルはイベントループが動いている間ずっと保持する
    // (dropすると再生が止まる、`event_loop.run`は戻らないため事実上
    // プロセス終了までスコープに残る)。
    let _bgm = audio::play_bgm_loop();

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
                    Some(VirtualKeyCode::Space) => state.fire_held = pressed,
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
    enemy_descriptor_set: vk::DescriptorSet,
    boss_descriptor_set: vk::DescriptorSet,
    reticle_descriptor_set: vk::DescriptorSet,
    ship_descriptor_set: vk::DescriptorSet,
    floor_a_descriptor_set: vk::DescriptorSet,
    floor_b_descriptor_set: vk::DescriptorSet,
    bullet_descriptor_set: vk::DescriptorSet,
    explosion_descriptor_set: vk::DescriptorSet,
    hud_descriptor_set: vk::DescriptorSet,
    vertex_buffer: vk::Buffer,
    vertex_buffer_memory: vk::DeviceMemory,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    image_available: vk::Semaphore,
    render_finished: vk::Semaphore,
    in_flight: vk::Fence,
    _enemy_texture_image: vk::Image,
    _boss_texture_image: vk::Image,
    _reticle_texture_image: vk::Image,
    _ship_texture_image: vk::Image,
    _floor_a_texture_image: vk::Image,
    _floor_b_texture_image: vk::Image,
    _bullet_texture_image: vk::Image,
    _explosion_texture_image: vk::Image,
    _hud_texture_image: vk::Image,
    sampler: vk::Sampler,
}

/// 固定スプライト(敵・照準・自機)+床`FLOOR_STRIPES`本+弾
/// `MAX_BULLETS`発+爆発`MAX_EXPLOSIONS`個+HUDバナー1枚を毎フレーム描画する。
const FIXED_SPRITE_COUNT: usize = 3;
const FLOOR_SPRITE_OFFSET: usize = FIXED_SPRITE_COUNT;
const BULLET_SPRITE_OFFSET: usize = FLOOR_SPRITE_OFFSET + FLOOR_STRIPES;
const EXPLOSION_SPRITE_OFFSET: usize = BULLET_SPRITE_OFFSET + MAX_BULLETS;
const HUD_SPRITE_INDEX: usize = EXPLOSION_SPRITE_OFFSET + MAX_EXPLOSIONS;
const DRAWN_SPRITE_COUNT: usize = HUD_SPRITE_INDEX + 1;
/// 一意のテクスチャ数(敵・照準・自機・床2色・弾・爆発・HUDバナー)。
const TEXTURE_COUNT: usize = FIXED_SPRITE_COUNT + 6;
const VERTS_PER_SPRITE: usize = 6;
const HUD_TEXT: &str = "ARROWS/WASD MOVE  SPACE FIRE  ESC/X CLOSE";
/// HUDバナーの画面下からの高さ(NDC単位)。
const HUD_HEIGHT_NDC: f32 = 0.08;

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
        let window_size = window.inner_size();
        let extent = vk::Extent2D { width: window_size.width.max(1), height: window_size.height.max(1) };
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

        let (enemy_image, enemy_view) =
            create_texture_from_rgba(&device, &memory_properties, queue, queue_family_index, &sprite::rasterize(sprite::ENEMY, sprite::enemy_palette, 2))?;
        let (boss_image, boss_view) =
            create_texture_from_rgba(&device, &memory_properties, queue, queue_family_index, &sprite::rasterize(sprite::BOSS, sprite::boss_palette, 2))?;
        let (reticle_image, reticle_view) = create_solid_texture(&device, &memory_properties, queue, queue_family_index, Rgba8 { r: 240, g: 220, b: 30, a: 255 })?;
        let (ship_image, ship_view) =
            create_texture_from_rgba(&device, &memory_properties, queue, queue_family_index, &sprite::rasterize(sprite::SHIP, sprite::ship_palette, 2))?;
        let (floor_a_image, floor_a_view) = create_solid_texture(&device, &memory_properties, queue, queue_family_index, Rgba8 { r: 30, g: 140, b: 40, a: 255 })?;
        let (floor_b_image, floor_b_view) = create_solid_texture(&device, &memory_properties, queue, queue_family_index, Rgba8 { r: 15, g: 100, b: 25, a: 255 })?;
        let (bullet_image, bullet_view) = create_solid_texture(&device, &memory_properties, queue, queue_family_index, Rgba8 { r: 255, g: 255, b: 255, a: 255 })?;
        let (explosion_image, explosion_view) = create_solid_texture(&device, &memory_properties, queue, queue_family_index, Rgba8 { r: 250, g: 140, b: 20, a: 255 })?;
        let hud_texture = font::render_text_banner(HUD_TEXT, 3, Rgba8 { r: 255, g: 255, b: 255, a: 255 }, Rgba8 { r: 10, g: 10, b: 15, a: 255 });
        let (hud_image, hud_view) = create_texture_from_rgba(&device, &memory_properties, queue, queue_family_index, &hud_texture)?;

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

        let pool_size = vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: TEXTURE_COUNT as u32 };
        let pool_sizes = [pool_size];
        let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder().pool_sizes(&pool_sizes).max_sets(TEXTURE_COUNT as u32);
        let descriptor_pool = device.create_descriptor_pool(&descriptor_pool_info, None).map_err(|e| e.to_string())?;

        let set_layouts = vec![descriptor_set_layout; TEXTURE_COUNT];
        let ds_alloc_info = vk::DescriptorSetAllocateInfo::builder().descriptor_pool(descriptor_pool).set_layouts(&set_layouts);
        let sets = device.allocate_descriptor_sets(&ds_alloc_info).map_err(|e| e.to_string())?;
        let (
            enemy_descriptor_set,
            boss_descriptor_set,
            reticle_descriptor_set,
            ship_descriptor_set,
            floor_a_descriptor_set,
            floor_b_descriptor_set,
            bullet_descriptor_set,
            explosion_descriptor_set,
            hud_descriptor_set,
        ) = (sets[0], sets[1], sets[2], sets[3], sets[4], sets[5], sets[6], sets[7], sets[8]);

        for (set, view) in [
            (enemy_descriptor_set, enemy_view),
            (boss_descriptor_set, boss_view),
            (reticle_descriptor_set, reticle_view),
            (ship_descriptor_set, ship_view),
            (floor_a_descriptor_set, floor_a_view),
            (floor_b_descriptor_set, floor_b_view),
            (bullet_descriptor_set, bullet_view),
            (explosion_descriptor_set, explosion_view),
            (hud_descriptor_set, hud_view),
        ] {
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

        let vbuf_size = ((DRAWN_SPRITE_COUNT * VERTS_PER_SPRITE) * std::mem::size_of::<Vertex>()) as vk::DeviceSize;
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
            enemy_descriptor_set,
            boss_descriptor_set,
            reticle_descriptor_set,
            ship_descriptor_set,
            floor_a_descriptor_set,
            floor_b_descriptor_set,
            bullet_descriptor_set,
            explosion_descriptor_set,
            hud_descriptor_set,
            vertex_buffer,
            vertex_buffer_memory,
            command_pool,
            command_buffer,
            image_available,
            render_finished,
            in_flight,
            _enemy_texture_image: enemy_image,
            _boss_texture_image: boss_image,
            _reticle_texture_image: reticle_image,
            _ship_texture_image: ship_image,
            _floor_a_texture_image: floor_a_image,
            _floor_b_texture_image: floor_b_image,
            _bullet_texture_image: bullet_image,
            _explosion_texture_image: explosion_image,
            _hud_texture_image: hud_image,
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

        let mut sprites_ndc = [[0.0f32; 4]; DRAWN_SPRITE_COUNT];
        sprites_ndc[0] = state.enemy_dest_ndc();
        sprites_ndc[1] = state.reticle_dest_ndc();
        sprites_ndc[2] = state.ship_dest_ndc();
        for i in 0..FLOOR_STRIPES {
            sprites_ndc[FLOOR_SPRITE_OFFSET + i] = state.floor_stripe_dest_ndc(i);
        }
        for i in 0..MAX_BULLETS {
            sprites_ndc[BULLET_SPRITE_OFFSET + i] = state.bullet_dest_ndc(i);
        }
        for i in 0..MAX_EXPLOSIONS {
            sprites_ndc[EXPLOSION_SPRITE_OFFSET + i] = state.explosion_dest_ndc(i);
        }
        sprites_ndc[HUD_SPRITE_INDEX] = [-1.0, 1.0 - HUD_HEIGHT_NDC, 1.0, 1.0];
        let mut all_verts = [Vertex { pos: [0.0; 3], uv: [0.0; 2] }; DRAWN_SPRITE_COUNT * VERTS_PER_SPRITE];
        for (i, ndc) in sprites_ndc.iter().enumerate() {
            let verts = quad_vertices(*ndc);
            all_verts[i * VERTS_PER_SPRITE..(i + 1) * VERTS_PER_SPRITE].copy_from_slice(&verts);
        }
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

        let mut sets = [self.enemy_descriptor_set; DRAWN_SPRITE_COUNT];
        sets[0] = if state.is_boss { self.boss_descriptor_set } else { self.enemy_descriptor_set };
        sets[1] = self.reticle_descriptor_set;
        sets[2] = self.ship_descriptor_set;
        for i in 0..FLOOR_STRIPES {
            sets[FLOOR_SPRITE_OFFSET + i] = if i % 2 == 0 { self.floor_a_descriptor_set } else { self.floor_b_descriptor_set };
        }
        for i in 0..MAX_BULLETS {
            sets[BULLET_SPRITE_OFFSET + i] = self.bullet_descriptor_set;
        }
        for i in 0..MAX_EXPLOSIONS {
            sets[EXPLOSION_SPRITE_OFFSET + i] = self.explosion_descriptor_set;
        }
        sets[HUD_SPRITE_INDEX] = self.hud_descriptor_set;
        for (i, &set) in sets.iter().enumerate() {
            self.device.cmd_bind_descriptor_sets(self.command_buffer, vk::PipelineBindPoint::GRAPHICS, self.pipeline_layout, 0, &[set], &[]);
            self.device.cmd_draw(self.command_buffer, VERTS_PER_SPRITE as u32, 1, (i * VERTS_PER_SPRITE) as u32, 0);
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
    create_texture_from_rgba(device, memory_properties, queue, queue_family_index, &TextureRgba8 { width: 1, height: 1, pixels: vec![color] })
}

/// 任意サイズのRGBA8テクスチャを実際にアップロードする(`create_solid_texture`
/// の1x1専用実装を一般化したもの、HUDバナー画像等の任意画像に使う)。
unsafe fn create_texture_from_rgba(
    device: &ash::Device,
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    queue: vk::Queue,
    queue_family_index: u32,
    texture: &TextureRgba8,
) -> Result<(vk::Image, vk::ImageView), String> {
    const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;
    let (width, height) = (texture.width, texture.height);

    let tex_bytes_size = (width as u64) * (height as u64) * 4;
    let (staging_buffer, staging_memory) = create_host_visible_buffer(device, memory_properties, tex_bytes_size, vk::BufferUsageFlags::TRANSFER_SRC)?;
    let ptr = device.map_memory(staging_memory, 0, tex_bytes_size, vk::MemoryMapFlags::empty()).map_err(|e| e.to_string())?;
    let dst = std::slice::from_raw_parts_mut(ptr as *mut u8, tex_bytes_size as usize);
    for (i, p) in texture.pixels.iter().enumerate() {
        dst[i * 4..i * 4 + 4].copy_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    device.unmap_memory(staging_memory);

    let image_info = vk::ImageCreateInfo::builder()
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D { width, height, depth: 1 })
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
        image_extent: vk::Extent3D { width, height, depth: 1 },
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
