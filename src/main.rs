use eframe::egui;
use std::sync::{Arc, Mutex};
use image::GenericImageView;
use rfd::FileDialog;
//use std::process::Command;
use rusqlite::{params, Connection};
use std::os::unix::fs::PermissionsExt;
mod ui_utils;


#[derive(PartialEq, Clone, Copy)]
enum OcrState {
    Idle,           // 等待扫描
    Initializing,   // 正在初始化显存
    Processing,     // 正在识别
    Success,        // 识别成功
    Failure,        // 识别失败
		Translating,
}
pub struct OcrResources {
    pub cli_exe: std::path::PathBuf,      // llama-cli (用于翻译)
    pub mtmd_exe: std::path::PathBuf,     // llama-mtmd-cli (用于 OCR)
    pub text_model: std::path::PathBuf,   // Gemma-3 模型
    pub v_model: std::path::PathBuf,      // GLM-OCR 模型
    pub mmproj: std::path::PathBuf,       // GLM-OCR 的视觉插件
		pub ocr_prompt: String,
    pub trans_prompt: String,
}
fn default_ocr_prompt() -> String {
	"Extract text and use LaTeX for all mathematical formulas from the image.".to_string()
}
fn default_trans_prompt() -> String {
	"Translate this Chinese text into English, do not solve it, formatted with LaTeX for mathematical expressions: ".to_string()
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct AppConfig {
    cli_exe: String,
    mtmd_exe: String,
    text_model: String,
    v_model: String,
    mmproj: String,
		ocr_prompt: String,
    trans_prompt: String,
}

impl Default for AppConfig {
	fn default() -> Self {
		Self {
			cli_exe: "llama-cli".into(),
			mtmd_exe: "llama-mtmd-cli".into(),
			text_model: "gemma-3-4b-it-Q4_K_M.gguf".into(),
			v_model: "GLM-OCR-Q8_0.gguf".into(),
			mmproj: "mmproj-GLM-OCR-Q8_0.gguf".into(),
			ocr_prompt: default_ocr_prompt(),
			trans_prompt: default_trans_prompt(),
		}
	}
}

struct HistoryEntry {
    id: i32,
    time: String,
    ocr_text: String,
    image_data: Vec<u8>,
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 加载文泉驿
    fonts.font_data.insert(
        "wqy".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/wqy-microhei.ttc")).into(),
    );

    // 将 WQY 插入到默认字体家族的末尾
    // 这样：'A' 会在默认字体里找到并显示；'中' 在默认字体里找不到，会跳到 WQY 里找
    fonts.families.get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .push("wqy".to_owned()); // 使用 .push() 放在最后

    fonts.families.get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("wqy".to_owned());

    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result<()> {
	let options = eframe::NativeOptions {
		viewport: egui::ViewportBuilder::default()
			.with_inner_size([800.0, 600.0])
			.with_decorations(false)
			.with_active(true),     
			..Default::default()
	};

	eframe::run_native(
		"OCR Tools",
		options,
		Box::new(|cc| {
			setup_fonts(&cc.egui_ctx);
			egui_extras::install_image_loaders(&cc.egui_ctx);
			Ok(Box::new(OCRTApp::default()))
		}),
	)
}
//字段
struct OCRTApp {
	app_title: String,
	ocr_content: Arc<Mutex<String>>, 
	translated_content: Arc<Mutex<String>>,
	state: Arc<Mutex<OcrState>>,
	selected_path: Arc<Mutex<String>>,
	is_editing: bool,
	is_chat_mode: bool,
	config: AppConfig,
	show_settings: bool,
	db_path: std::path::PathBuf,
	show_history: bool, 
	history_items: Vec<HistoryEntry>,
	preview_image: Option<(String, Vec<u8>)>,
}
//初始化
impl Default for OCRTApp {
	fn default() -> Self {
		let config: AppConfig = confy::load("ocr-tools", "config").unwrap_or_default();
		let db_path = if let Some(proj_dirs) = directories::ProjectDirs::from("", "", "ocr-tools") {
			proj_dirs.data_dir().join("history.db")
		} else {
			std::path::PathBuf::from("history.db")
		};
		Self {
			app_title: "OCR Tools".to_owned(),
			//ocr_content: Arc::new(Mutex::new("等待扫描...".to_owned())),
			ocr_content: Arc::new(Mutex::new(String::new())),
			translated_content: Arc::new(Mutex::new(String::new())),
			state: Arc::new(Mutex::new(OcrState::Idle)),
			selected_path: Arc::new(Mutex::new("".to_owned())),
			is_editing: false,
			is_chat_mode: false,
			config,
			show_settings: false,
			db_path,
			show_history: false,
			history_items: Vec::new(),
			preview_image: None,
		}
	}
}
//循环
impl eframe::App for OCRTApp {
	fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		// 调用独立出来的标题栏函数
		self.render_custom_title_bar(ctx);

		// 渲染主体内容
		self.draw_central_panel(ctx);

		self.draw_settings_window(ctx);

		self.draw_history_window(ctx);


		if !ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
			self.draw_window_frame(ctx);
		}
	}
}
//中央面板
impl OCRTApp {
	fn draw_central_panel(&mut self, ctx: &egui::Context) {
		// 1. 处理文件拖拽（后台逻辑，无 UI 占用）
		self.handle_file_drag_and_drop(ctx);

		egui::CentralPanel::default().show(ctx, |ui| {
			// --- 第一部分：顶部工具栏 ---
			self.draw_top_toolbar(ui, ctx);

			// --- 第二部分：状态条（路径显示与 Spinner） ---
			// 修正：确保在这里明确调用 draw_status_bar
			self.draw_status_bar(ui);

			// --- 第三部分：主内容展示区 ---
			self.draw_content_area(ui);
		});
	}
}
//处理文件拖拽
impl OCRTApp {
	fn handle_file_drag_and_drop(&mut self, ctx: &egui::Context) {
		if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
			let dropped = ctx.input(|i| i.raw.dropped_files.clone());
			for file in dropped {
				if let Some(path) = file.path {
					let path_str = path.display().to_string();
					let lower = path_str.to_lowercase();
					if [".png", ".jpg", ".jpeg", ".webp"].iter().any(|ext| lower.ends_with(ext)) {
						self.trigger_ocr(ctx.clone(), path_str);
						break;
					}
				}
			}
		}
	}
}
//顶部工具栏
impl OCRTApp {
    fn draw_top_toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let current_state = *self.state.lock().unwrap();
        let ocr_res = self.ocr_content.lock().unwrap().clone();
        let trans_res = self.translated_content.lock().unwrap().clone();

        let can_pick = current_state == OcrState::Idle 
            || current_state == OcrState::Success 
            || current_state == OcrState::Failure;

        let is_busy = current_state == OcrState::Initializing 
            || current_state == OcrState::Processing 
            || current_state == OcrState::Translating;

        let has_content = !ocr_res.is_empty();

        ui.horizontal(|ui| {
            // 设置组件间距为传统的默认/规范间距
            ui.spacing_mut().item_spacing.x = 10.0;

            // 1. 选择图片
            if ui.add_enabled(can_pick, egui::Button::new("选择图片")).clicked() {
                if let Some(path) = FileDialog::new()
                    .add_filter("图片文件", &["png", "jpg", "jpeg", "bmp", "webp"])
                    .pick_file() 
                {
                    self.trigger_ocr(ctx.clone(), path.display().to_string());
                }
            }

            ui.separator(); // 添加分隔线划分功能区域

            // 2. 粘贴图片
						let paste_btn = egui::Button::new(
							egui::RichText::new("📋 粘贴图片")
							.color(egui::Color32::WHITE)
							.strong()
						)
							.fill(egui::Color32::from_rgb(45, 105, 175)); // 使用经典高亮蓝色，突出常用功能
            if ui.add_enabled(can_pick, paste_btn).clicked() {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    if let Ok(image) = cb.get_image() {
                        let temp_path = std::env::temp_dir().join("ocr_manual_paste.png");
                        let save_result = image::save_buffer(
                            &temp_path,
                            &image.bytes,
                            image.width as u32,
                            image.height as u32, 
                            image::ColorType::Rgba8,
                        );
                        if let Err(e) = save_result {
                            eprintln!("粘贴图片保存失败: {}", e);
                            if let Ok(mut state) = self.state.lock() {
                                *state = OcrState::Failure;
                            }
                        } else {
                            self.trigger_ocr(ctx.clone(), temp_path.display().to_string());
                        }
                    } 
                }
            }

            ui.separator(); // 添加分隔线划分功能区域

            // 3. 翻译结果
            let trans_btn = egui::Button::new("翻译成英文")
                .fill(egui::Color32::from_rgb(45, 70, 90));
            if ui.add_enabled(has_content && !is_busy, trans_btn).clicked() {
                self.is_editing = false;
                self.run_translate_task(ctx.clone(), ocr_res.clone());
            }

            ui.separator(); // 添加分隔线划分功能区域

            // 4. 编辑/阅读切换
            let edit_label = if self.is_editing { "阅读模式" } else { "编辑模式" };
            if ui.add_enabled(!is_busy, egui::Button::new(edit_label)).clicked() {
                self.is_editing = !self.is_editing;
            }

            ui.separator(); // 添加分隔线划分功能区域

            // 5. 复制全部结果
						let copy_btn = egui::Button::new(
							egui::RichText::new("📋复制全部")
							.color(egui::Color32::WHITE)
							.strong()
						)
							.fill(egui::Color32::from_rgb(40, 110, 80));
            if ui.add_enabled(has_content, copy_btn).clicked() {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let full_text = if trans_res.is_empty() {
                        ocr_res.clone()
                    } else {
                        format!("{}\n\n---\n\n{}", ocr_res, trans_res)
                    };
                    let _ = cb.set_text(full_text);
                }
            }


            ui.separator(); // 添加分隔线

            // 6. 清空结果
            let clear_btn = egui::Button::new("清空结果")
                .fill(egui::Color32::from_rgb(100, 50, 50));
            if ui.add_enabled(!is_busy, clear_btn).clicked() {
                *self.ocr_content.lock().unwrap() = String::new();
                *self.translated_content.lock().unwrap() = String::new();
                *self.selected_path.lock().unwrap() = String::new();
                *self.state.lock().unwrap() = OcrState::Idle;
                self.is_editing = false;
            }

            ui.separator(); // 辅助功能分隔线

            // 7. 对话模式切换
            let (btn_text, btn_color) = if self.is_chat_mode {
                ("退出对话", egui::Color32::from_rgb(70, 70, 70))
            } else {
                ("AI对话", egui::Color32::from_rgb(45, 90, 70))
            };
            if ui.add_enabled(!is_busy, egui::Button::new(btn_text).fill(btn_color)).clicked() {
                self.is_chat_mode = !self.is_chat_mode;
            }

            ui.separator(); // 添加分隔线

            // 8. 历史记录
            if ui.button("历史记录").clicked() {
                self.load_history();
                self.show_history = !self.show_history;
            }


            // 9. 右侧对齐的设置按钮
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(egui::RichText::new("⚙")).clicked() {
                    self.show_settings = !self.show_settings;
                }
            });
        });
    }
}
// 状态与路径显示条
impl OCRTApp {
	fn draw_status_bar(&mut self, ui: &mut egui::Ui) {
		let current_state = *self.state.lock().unwrap();
		let path_show = self.selected_path.lock().unwrap();

		ui.separator();

		if !path_show.is_empty() || current_state == OcrState::Initializing || current_state == OcrState::Processing {
			ui.horizontal(|ui| {
				if !path_show.is_empty() {
					ui.label(egui::RichText::new(format!("当前图片: {}", path_show))
						.small()
						.color(egui::Color32::GRAY));
				}

				ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
					match current_state {
						OcrState::Initializing => {
							ui.add(egui::Spinner::new().size(12.0));
							ui.label(egui::RichText::new("正在唤醒引擎...").small().color(egui::Color32::LIGHT_BLUE));
						}
						OcrState::Processing => {
							ui.add(egui::Spinner::new().size(12.0));
							ui.label(egui::RichText::new("正在推理中...").small().color(egui::Color32::GOLD));
						}
						OcrState::Translating => {
							ui.add(egui::Spinner::new().size(12.0));
							ui.label(egui::RichText::new("正在翻译中...").small().color(egui::Color32::GOLD));
						}
						_ => {}
					}
				});
			});
			ui.separator();
		}
	}
}
//主内容展示区
impl OCRTApp {
	fn draw_content_area(&mut self, ui: &mut egui::Ui) {
		let current_state = *self.state.lock().unwrap();
		let ocr_res = self.ocr_content.lock().unwrap().clone();
		let trans_res = self.translated_content.lock().unwrap().clone();

		let text_color = match current_state {
			OcrState::Failure => egui::Color32::LIGHT_RED,
			_ => egui::Color32::from_rgb(220, 220, 220),
		};

		egui::Frame::NONE
			.inner_margin(egui::Margin::symmetric(10, 15)) 
			.show(ui, |ui| {
				egui::ScrollArea::vertical()
					.auto_shrink([false; 2]) 
					.show(ui, |ui| {
						if self.is_chat_mode {
							egui::Frame::group(ui.style()).show(ui, |ui| {
								ui.heading("📂 可用模型列表 (~/.local/share/llama_models)");
								ui.add_space(10.0);

								let models = get_model_list();
								if models.is_empty() {
									ui.label(egui::RichText::new("未发现 .gguf 模型文件").color(egui::Color32::GRAY));
								}

								egui::ScrollArea::vertical().show(ui, |ui| {
									for path in models {
										let file_name = path.file_name()
											.map(|f| f.to_string_lossy().to_string()) 
											.unwrap_or_else(|| "未知模型".to_string());

										ui.horizontal(|ui| {
											ui.label(egui::RichText::new("🤖").size(18.0));
											ui.vertical(|ui| {
												ui.label(egui::RichText::new(&file_name).strong());
												ui.label(egui::RichText::new(path.to_string_lossy()).small().weak());
											});

											ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
												if ui.button("🚀 启动").clicked() {
													//let res = get_ocr_resources().unwrap();
													//launch_in_terminal(&res.cli_exe.display().to_string(), &path);
													match get_ocr_resources_from_config(&self.config) {
														Ok(res) => {
															launch_in_terminal(&res.cli_exe.display().to_string(), &path);
														}
														Err(e) => {
															eprintln!("无法启动终端: {}", e);
														}
													}
												}
											});
										});
										ui.separator();
									}
								});
							});

						} else if self.is_editing {
							// --- 原始编辑模式逻辑 ---
							let total_available = ui.available_height();
							let spacing_reserved = 80.0; 
							let half_height = if !trans_res.is_empty() {
								((total_available - spacing_reserved) / 2.0).max(100.0)
							} else {
								total_available - 40.0
							};
							if let (Ok(mut ocr_content), Ok(mut trans_content)) = 
								(self.ocr_content.lock(), self.translated_content.lock()) 
							{
								ui.label(egui::RichText::new("OCR原文").small().color(egui::Color32::GRAY));
								ui.add(
									egui::TextEdit::multiline(&mut *ocr_content)
									.font(egui::TextStyle::Monospace)
									.desired_width(f32::INFINITY)
									.min_size(egui::vec2(0.0, half_height))
									.margin(egui::Margin::same(8))
								);

								if !trans_content.is_empty() {
									ui.add_space(15.0);
									ui.label(egui::RichText::new("翻译结果").small().color(egui::Color32::GRAY));
									ui.add(
										egui::TextEdit::multiline(&mut *trans_content)
										.font(egui::TextStyle::Monospace)
										.desired_width(f32::INFINITY)
										.min_size(egui::vec2(0.0, half_height))
										.margin(egui::Margin::same(8))
									);
								}
							}
						} else {
							// --- 原始阅读模式逻辑 ---
							let ocr_res_ui = ui.add(
								egui::Label::new(
									egui::RichText::new(&ocr_res)
									.monospace()
									.size(15.0)
									.color(text_color)
								)
								.wrap()
								.selectable(true)
							);

							let mut trans_res_ui = None;
							if !trans_res.is_empty() {
								ui.add_space(20.0);
								ui.separator();
								ui.add_space(20.0);
								trans_res_ui = Some(ui.add(
										egui::Label::new(
											egui::RichText::new(&trans_res)
											.monospace()
											.size(15.0)
											.color(egui::Color32::LIGHT_BLUE)
										)
										.wrap()
										.selectable(true)
								));
							} else if ocr_res.is_empty() {
								ui.label(egui::RichText::new("请点击上方按钮选择一张图片").weak());
							}

							// --- 原始自定滚动逻辑 ---
							let is_being_dragged = ocr_res_ui.dragged() 
								|| trans_res_ui.map_or(false, |r| r.dragged());
							if is_being_dragged {
								if let Some(pointer_pos) = ui.ctx().pointer_interact_pos() {
									let scroll_rect = ui.clip_rect();
									let margin = 35.0; 
									let mut scroll_delta = 0.0;
									if pointer_pos.y < (scroll_rect.min.y + margin) {
										let dist = (scroll_rect.min.y + margin - pointer_pos.y).max(2.0);
										scroll_delta = dist * 2.5; 
									} else if pointer_pos.y > (scroll_rect.max.y - margin) {
										let dist = (pointer_pos.y - (scroll_rect.max.y - margin)).max(2.0);
										scroll_delta = -dist * 2.5; 
									}
									if scroll_delta != 0.0 {
										ui.scroll_with_delta(egui::vec2(0.0, scroll_delta));
									}
								}
							}
							if current_state == OcrState::Initializing || current_state == OcrState::Processing {
								ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
							}
						}
					});
			});
	}
}

impl OCRTApp {
	fn trigger_ocr(&mut self, ctx: egui::Context, path_str: String) {
		// 0. 预处理：生成缩放后的临时图片路径（失败则回退到原图）
    let target_path = preprocess_image(&path_str).unwrap_or_else(|_| path_str.clone());

		// 1. 更新路径
		if let Ok(mut p) = self.selected_path.lock() { 
			*p = path_str.clone(); 
		}
		// 2. 状态切换
		if let Ok(mut s) = self.state.lock() { *s = OcrState::Initializing; }
		// 3. 清空内容
		if let Ok(mut c) = self.ocr_content.lock() { c.clear(); }
		// 4. 清空翻译内容
    if let Ok(mut t) = self.translated_content.lock() { *t = String::new(); }
		// 5. 启动异步任务
		self.run_ocr_task(ctx, target_path);
	}
}

//调用llama-mtmd-cli
impl OCRTApp {
	fn run_ocr_task(&self, ctx: egui::Context, img_path: String) {
		let content_ptr = self.ocr_content.clone();
		let state_ptr = self.state.clone();
		let config_for_thread = self.config.clone();
		let db_p = self.db_path.clone();

		if let Ok(mut s) = state_ptr.lock() { *s = OcrState::Initializing; }

		std::thread::spawn(move || {
			let result = (|| -> anyhow::Result<String> {

				// 1. 获取统一的资源路径
				let res = get_ocr_resources_from_config(&config_for_thread)?;

				if let Ok(mut s) = state_ptr.lock() { *s = OcrState::Processing; }

				// 建议增加一个更明确的系统提示
				//let prompt = "Extract text and use LaTeX for all mathematical formulas from the image.";

				// 2. 调用新编译的 mtmd 引擎
				let output = std::process::Command::new(&res.mtmd_exe) // 使用 mtmd_exe
					//.env("GGML_VK_VISIBLE_DEVICES", "0")
					.arg("-m").arg(&res.v_model)                    // 使用视觉大模型
					.arg("--jinja")
					.arg("--mmproj").arg(&res.mmproj)                // 使用视觉适配器
					.arg("--image").arg(&img_path)
					//.arg("-ngl").arg("100")                          // 尝试全量卸载到 GPU
					.arg("--temp").arg("0.1")       
					//.arg("-p").arg(prompt) 
					.arg("-p").arg(&res.ocr_prompt)
					.arg("-n").arg("4096")
					.arg("-c").arg("16384")
					.stderr(std::process::Stdio::piped())
					.output()?; // 阻塞等待 OCR 结束

				if !output.status.success() {
					let err_msg = String::from_utf8_lossy(&output.stderr);
					return Err(anyhow::anyhow!("OCR 引擎执行失败: {}", err_msg));
				}

				let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();

				if raw.is_empty() {
					return Err(anyhow::anyhow!("模型完全没有输出任何内容"));
				}

				Ok(raw)
			})();

			if let (Ok(mut c), Ok(mut s)) = (content_ptr.lock(), state_ptr.lock()) {
				match result {
					Ok(text) => {
						*c = text.clone();
						*s = OcrState::Success; 
						let _ = save_history_to_db_static(&db_p, &text, "", &img_path);
					},
					Err(e) => {
						*c = format!("❌ 错误: {}", e);
						*s = OcrState::Failure;
					}
				}
			}

			ctx.request_repaint();
		});
	}
}
//翻译
impl OCRTApp {
	fn run_translate_task(&self, ctx: egui::Context, source_text: String) {
		let trans_ptr = self.translated_content.clone();
		let state_ptr = self.state.clone();
		let config_for_thread = self.config.clone();

		if let Ok(mut s) = state_ptr.lock() { *s = OcrState::Translating; }
		std::thread::spawn(move || {
			let result = (|| -> anyhow::Result<String> {
				//let res = get_ocr_resources()?;
				let res = get_ocr_resources_from_config(&config_for_thread)?;

				//let prompt_text = format!(
				//	"Translate this Chinese text into English, do not solve it, formatted with LaTeX for mathematical expressions: \"{}\"", 
				//	source_text.replace('\n', " ")
				//);
				let prompt_text = format!(
					"{} \"{}\"", 
					res.trans_prompt, 
					source_text.replace('\n', " ")
				);

				// 1. 配置命令：强制静默模式
				let output = std::process::Command::new(&res.cli_exe)
					.arg("-m").arg(&res.text_model)
					.arg("-n").arg("4096")
					.arg("-c").arg("16384")
					.arg("-ngl").arg("100") 
					.arg("--jinja")
					.arg("--reasoning").arg("off")
					.arg("--log-disable")
					.arg("--no-display-prompt") // 不回显 Prompt
					.arg("--simple-io")         // 禁用 TTY 交互
					.arg("--single-turn")       // 运行完立即退出
					.arg("-p").arg(&prompt_text)
					.stdout(std::process::Stdio::piped()) // 捕获输出
					.stderr(std::process::Stdio::piped())
					.output()?;

				if !output.status.success() {
					return Err(anyhow::anyhow!("进程异常退出: {:?}", output.status.code()));
				}

				// 2. 转换输出
				let raw = String::from_utf8_lossy(&output.stdout);

				// 3. 清洗逻辑：由于禁用了 Prompt 回显，这里拿到的应该是纯结果
				let clean_res = raw.lines()
					.filter(|line| {
						let l = line.trim();
						// 如果行是空的，或者是以下这些垃圾字符开头的，全部剔除
						!l.is_empty() && 
							!l.starts_with(">") &&
							!l.starts_with("Loading model") &&
							!l.starts_with("build      :") &&
							!l.starts_with("model      :") &&
							!l.starts_with("modalities :") &&
							!l.contains("available commands:") &&
							!l.starts_with("/") &&             // 过滤掉 /exit, /regen, /clear 等指令行
							!l.contains("Ctrl+C") &&           // 过滤掉包含 Ctrl+C 的那行帮助
							!l.starts_with("[") &&             // 过滤性能统计 [ Prompt: ... ]
							!l.contains("Exiting")             // 过滤退出提示
					})
				.collect::<Vec<_>>()
					.join("\n")
					.trim()
					.to_string();

				if clean_res.is_empty() {
					return Err(anyhow::anyhow!("内容提取失败。Raw: {}", raw));
				}

				Ok(clean_res)
			})();


			// 4. 更新 UI 状态
			if let (Ok(mut c), Ok(mut s)) = (trans_ptr.lock(), state_ptr.lock()) {
				match result {
					Ok(text) => { *c = text; *s = OcrState::Success; }
					Err(e) => { *c = format!("❌ 错误: {}", e); *s = OcrState::Failure; }
				}
			}
			ctx.request_repaint();
		});
	}
}

//压缩图片
fn preprocess_image(input_path: &str) -> anyhow::Result<String> {
    let img = image::open(input_path)?;
    let (width, height) = img.dimensions();

    // 核心参数设置
    let max_dimension = 1340.0;   // 正常的长边限制
    let min_width_limit = 890.0;  // 强制保留的最小宽度（生命线）

    // 判断逻辑：只有当图片确实“大”到需要缩放时才进入
    if width as f32 > max_dimension || height as f32 > max_dimension {
        
        // 1. 计算初步缩放比例（常规长边缩放）
        let mut scale = (max_dimension / width as f32).min(max_dimension / height as f32);
        
        // 2. 【核心修正】针对细长条图片的特殊照顾：
        // 如果缩放后的宽度比“生命线”还窄，且原图宽度本来就够，我们就放宽限制
        let projected_width = width as f32 * scale;
        if projected_width < min_width_limit && width as f32 > min_width_limit {
            // 重新计算比例，确保宽度正好等于生命线
            scale = min_width_limit / width as f32;
        }

        // 3. 执行缩放（如果 scale 还是小于 1.0）
        if scale < 1.0 {
            let new_width = (width as f32 * scale) as u32;
            let new_height = (height as f32 * scale) as u32;

						println!("📏 图片太大 ({}x{}), 正在缩放到 ({}x{})...", width, height, new_width, new_height);

            // 针对文字建议使用 CatmullRom 或 Lanczos3，效果比 Triangle 锐利很多
            let scaled_img = img.resize(new_width, new_height, image::imageops::FilterType::CatmullRom);

            let temp_path = std::env::temp_dir().join("ocr_resized_tmp.jpg");
            scaled_img.save(&temp_path)?;
            return Ok(temp_path.display().to_string());
        }
    }

    Ok(input_path.to_string())
}

fn get_model_list() -> Vec<std::path::PathBuf> {
	let home = std::env::var("HOME").unwrap_or_default();
	let model_dir = std::path::Path::new(&home).join(".local/share/llama_models");

	if let Ok(entries) = std::fs::read_dir(model_dir) {
		entries.filter_map(|e| e.ok())
			.map(|e| e.path())
			.filter(|p| p.extension().map_or(false, |ext| ext == "gguf"))
			.collect()
	} else {
		vec![]
	}
}
fn launch_in_terminal(cli_path: &str, model_path: &std::path::Path) {
    let model_str = model_path.to_string_lossy();
    let exec_str = format!("'{}' -m '{}' -cnv --color on --reasoning off; exec zsh", cli_path, model_str);
    let mut cmd = std::process::Command::new("mate-terminal");
    cmd.args([ "--", "zsh", "-lc", &exec_str ]);
		let _ = cmd.spawn();
}

//设置
impl OCRTApp {
	fn draw_settings_window(&mut self, ctx: &egui::Context) {
		if !self.show_settings { return; }
		let mut is_open = self.show_settings;
		let mut save_clicked = false;
		// 提前获取家目录
		let home = dirs::home_dir().unwrap_or_default();
		let bin_user = home.join(".local/bin");
		let bin_system = std::path::PathBuf::from("/usr/local/bin");

		// 确定 CLI 工具的默认打开目录：优先家目录，其次系统路径，最后当前目录
		let default_bin_dir = if bin_user.exists() {
			bin_user
		} else if bin_system.exists() {
			bin_system
		} else {
			std::env::current_dir().unwrap_or_default()
		};

		// 模型默认目录
		let default_model_dir = home.join(".local/share/llama_models");

		egui::Window::new("⚙ 设置").open(&mut is_open).show(ctx, |ui| {
			ui.heading("llama-cli配置");

			ui.horizontal(|ui| {
				ui.label("文本 CLI:");
				ui.text_edit_singleline(&mut self.config.cli_exe);
				if ui.button("📁").clicked() {
					if let Some(path) = rfd::FileDialog::new()
						.set_directory(&default_bin_dir)
							.pick_file() {
								self.config.cli_exe = path.display().to_string();
					}
				}
			});

			ui.horizontal(|ui| {
				ui.label("视觉 CLI:");
				ui.text_edit_singleline(&mut self.config.mtmd_exe);
				if ui.button("📁").clicked() {
					if let Some(path) = rfd::FileDialog::new()
						.set_directory(&default_bin_dir)
							.pick_file() {
								self.config.mtmd_exe = path.display().to_string();
					}
				}
			});

			ui.separator();
			ui.heading("模型配置");

			ui.horizontal(|ui| {
				ui.label("翻译模型:");
				ui.text_edit_singleline(&mut self.config.text_model);
				if ui.button("📁").clicked() {
					if let Some(path) = rfd::FileDialog::new()
						.set_directory(&default_model_dir)
							.pick_file() {
								self.config.text_model = path.display().to_string();
					}
				}
			});

			ui.horizontal(|ui| {
				ui.label("OCR模型:");
				ui.text_edit_singleline(&mut self.config.v_model);
				if ui.button("📁").clicked() {
					if let Some(path) = rfd::FileDialog::new()
						.set_directory(&default_model_dir)
							.pick_file() {
								self.config.v_model = path.display().to_string();
					}
				}
			});

			ui.horizontal(|ui| {
				ui.label("视觉插件:");
				ui.text_edit_singleline(&mut self.config.mmproj);
				if ui.button("📁").clicked() {
					if let Some(path) = rfd::FileDialog::new()
						.set_directory(&default_model_dir)
							.pick_file() {
								self.config.mmproj = path.display().to_string();
					}
				}
			});

			ui.separator();
			ui.heading("提示词 (Prompt) 配置");

			ui.horizontal(|ui| {
				ui.label("OCR Prompt:");
				ui.text_edit_singleline(&mut self.config.ocr_prompt); 
			});

			ui.add_space(5.0);

			ui.horizontal(|ui| {
				ui.label("翻译 Prompt:");
				ui.text_edit_singleline(&mut self.config.trans_prompt);
			});

			ui.add_space(10.0);
			ui.vertical_centered(|ui| {
				if ui.button("保存并应用").clicked() {
					let _ = confy::store("ocr-tools", "config", &self.config);
					save_clicked = true;
				}
			});
		});
		if save_clicked {
			self.show_settings = false;
		} else {
			self.show_settings = is_open;
		}
	}
}
fn get_ocr_resources_from_config(config: &AppConfig) -> anyhow::Result<OcrResources> {
    // 1. 解析所有资源路径
    let res = OcrResources {
        cli_exe: resolve_executable(&config.cli_exe),
        mtmd_exe: resolve_executable(&config.mtmd_exe),
        text_model: resolve_model_path(&config.text_model),
        v_model: resolve_model_path(&config.v_model),
        mmproj: resolve_model_path(&config.mmproj),
				ocr_prompt: config.ocr_prompt.clone(),
				trans_prompt: config.trans_prompt.clone(),
    };

    // 2. 统一物理存在性检查
    let check_list = [
        (&res.cli_exe, "文本引擎 CLI"),
        (&res.mtmd_exe, "视觉引擎 CLI"),
        (&res.text_model, "翻译模型"),
        (&res.v_model, "OCR 模型"),
        (&res.mmproj, "视觉插件 (mmproj)"),
    ];

    for (path, name) in check_list {
			if path.as_os_str().is_empty() {
				return Err(anyhow::anyhow!("❌ {} 路径不能为空", name)); 
			}

			if !path.is_file() {
				return Err(anyhow::anyhow!("❌ 资源未找到或不是文件 [{}]\n路径: {:?}", name, path));
			}
		}

    Ok(res)
}
// 通用路径展开：处理 ~ 和 环境变量（如 $HOME）
fn expand_path(input: &str) -> std::path::PathBuf {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return std::path::PathBuf::new();
    }
    let expanded = shellexpand::full(trimmed).unwrap_or(std::borrow::Cow::Borrowed(trimmed));
    std::path::PathBuf::from(expanded.into_owned())
}
fn resolve_model_path(input: &str) -> std::path::PathBuf {
    let path = expand_path(input);

    // 1. 如果路径已经存在，直接返回
    if path.exists() {
        return path;
    }

    // 2. 如果路径不存在，且输入只是个文件名 (比如 "gemma-3b.gguf")
    // 或者用户写了 ./models/xxx.gguf 但相对于当前目录没找到
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or(input);

    // 尝试 A: 程序所在目录下的 models 文件夹 (推荐做法)
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let models_dir = parent.join("models").join(filename);
            if models_dir.exists() { return models_dir; }
            
            let same_dir = parent.join(filename);
            if same_dir.exists() { return same_dir; }
        }
    }

    // 尝试 B: 用户常用的模型存放点 (可选)
    let home = dirs::home_dir().unwrap_or_default();
    let common_dir = home.join(".local/share/llama_models").join(filename);
    if common_dir.exists() { return common_dir; }

    // 3. 实在找不到了，返回最初展开的结果，交给后续的 check_list 报错
    path
}

// 专门用于寻找可执行文件的逻辑
fn resolve_executable(name_or_path: &str) -> std::path::PathBuf {
	let name_trimmed = name_or_path.trim();

	// 1. 处理路径输入
	if name_trimmed.starts_with('/') || name_trimmed.starts_with("./") || name_trimmed.starts_with("../") {
		return expand_path(name_trimmed);
	}

	// 2. 收集所有候选目录 (使用 PathBuf 避免生命周期问题)
	let mut search_dirs = Vec::new();

	// A. 程序所在目录
	if let Ok(current_exe) = std::env::current_exe() {
		if let Some(parent) = current_exe.parent() {
			search_dirs.push(parent.to_path_buf());
		}
	}

	// B. 环境变量 PATH
	if let Ok(paths) = std::env::var("PATH") {
		for bin_dir in std::env::split_paths(&paths) {
			search_dirs.push(bin_dir);
		}
	}

	// C. 显式硬编码补丁 (使用 PathBuf)
	let home = dirs::home_dir().unwrap_or_default();
	search_dirs.push(std::path::PathBuf::from("/usr/local/bin"));
	search_dirs.push(std::path::PathBuf::from("/usr/bin"));
	search_dirs.push(std::path::PathBuf::from("/bin"));
	search_dirs.push(home.join(".local/bin"));

	// 3. 执行搜索
	for dir in search_dirs {
		let final_dir = if dir.to_string_lossy().contains('$') {
			expand_path(&dir.to_string_lossy())
		} else {
			dir
		};

		let full_path = final_dir.join(name_trimmed);
		if full_path.is_file() {
			#[cfg(unix)]
			{
				if let Ok(metadata) = full_path.metadata() {
					if metadata.permissions().mode() & 0o111 != 0 {
						return full_path;
					}
				}
			}
			#[cfg(not(unix))]
			return full_path;
		}
	}

	// 4. 兜底
	expand_path(name_trimmed)
}
//历史记录
fn save_history_to_db_static(
    db_path: &std::path::Path, 
    ocr: &str, 
    trans: &str, 
    img: &str // 这里参数名叫 img
) -> anyhow::Result<()> {
    // 1. 使用传入的 db_path，而不是 self.db_path
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // 2. 同样，这里直接用 db_path
    let mut conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", &"WAL")?;

    let tx = conn.transaction()?;

    tx.execute(
        "CREATE TABLE IF NOT EXISTS ocr_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            time TEXT,
            ocr_text TEXT,
            trans_text TEXT,
            image_data BLOB
        )",
        [],
    )?;

    // 3. 修复报错 E0425：这里使用参数名 img (或者把上面参数名改为 img_path)
    let img_bytes = std::fs::read(img)?; 

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    tx.execute(
        "INSERT INTO ocr_history (time, ocr_text, trans_text, image_data) VALUES (?1, ?2, ?3, ?4)",
        params![now, ocr, trans, img_bytes],
    )?;

    tx.execute(
        "DELETE FROM ocr_history WHERE id NOT IN (
            SELECT id FROM ocr_history ORDER BY id DESC LIMIT 2000
        )",
        [],
    )?;

    tx.commit()?;
    Ok(())
}
impl OCRTApp {
    fn load_history(&mut self) {
        if let Ok(conn) = Connection::open(&self.db_path) {
            let mut stmt = conn.prepare(
                "SELECT id, time, ocr_text, image_data FROM ocr_history ORDER BY id DESC LIMIT 2000"
            ).unwrap();

            let rows = stmt.query_map([], |row| {
                Ok(HistoryEntry {
                    id: row.get(0)?,
                    time: row.get(1)?,
                    ocr_text: row.get(2)?,
                    image_data: row.get(3)?,
                })
            }).unwrap();

            self.history_items.clear();
            for row in rows {
                if let Ok(entry) = row {
                    self.history_items.push(entry);
                }
            }
        }
    }
}
impl OCRTApp {
	fn draw_history_window(&mut self, ctx: &egui::Context) {
		// 1. 处理大图查看 (逻辑保持不变)
		let preview_data = self.preview_image.as_ref().map(|(t, b)| (t.clone(), b.clone()));
		let mut is_preview_open = self.preview_image.is_some();
		if let Some((time, img_bytes)) = preview_data {
			egui::Window::new(format!("原始图片 - {}", time))
				.id(egui::Id::new("img_full_view"))
				.open(&mut is_preview_open)
				.anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
				.show(ctx, |ui| {
					egui::ScrollArea::both().show(ui, |ui| {
						ui.add(egui::Image::from_bytes(format!("bytes://full_{}", time), img_bytes));
					});
				});
		}
		if !is_preview_open { self.preview_image = None; }

		// 2. 历史记录主面板
		if !self.show_history { return; }
		let window_size = egui::vec2(700.0, 400.0);
		let title = format!("📜 历史记录 ({})", self.history_items.len());
		egui::Window::new(title)
			.open(&mut self.show_history)
			.fixed_size(window_size)
			.resizable(false)
			.show(ctx, |ui| {
				if self.history_items.is_empty() {
					ui.label("暂无历史记录");
				}

				// 严格定义总行高（包含卡片和卡片下方的间距）
				let row_height = 130.0;
				let card_height = 120.0; 
				let total_rows = self.history_items.len();

				egui::ScrollArea::vertical().show_rows(ui, row_height, total_rows, |ui, row_range| {
					for i in row_range {
						let item = &self.history_items[i];

						// 使用 allocate_ui 或者是固定的 Frame，确保它申请的空间严格等于预设的高度
						// 这样滚动时，egui 计算的虚拟高度和实际渲染高度才能 100% 对齐
						ui.allocate_ui(egui::vec2(ui.available_width(), row_height), |ui| {

							egui::Frame::canvas(ui.style())
								.inner_margin(8.0)
								.show(ui, |ui| {
									// 强制卡片内容区高度
									ui.set_height(card_height);

									ui.horizontal_top(|ui| { 
										// --- 【左侧区域】：固定高度的文字滚动区 ---
										// 减去右侧图片（80px）和分割线、间距等，预留出安全宽度
										let text_area_width = ui.available_width() - 110.0;

										ui.allocate_ui(egui::vec2(text_area_width, card_height), |ui| {
											ui.vertical(|ui| {
												ui.set_min_width(text_area_width);
												ui.label(egui::RichText::new(&item.time).color(egui::Color32::LIGHT_BLUE).small());
												ui.add_space(2.0);

												// 子滚动区高度减去上面时间 label 占用的空间（约 15-20px），防止再次撑开
												egui::ScrollArea::vertical()
													.id_salt(format!("scroll_{}", item.id))
													.max_height(card_height - 20.0) 
													.auto_shrink([false, true])
													.show(ui, |ui| {
														ui.add(egui::Label::new(egui::RichText::new(&item.ocr_text).weak())
															.selectable(true)
															.wrap()
														);
													});
											});
										});

										ui.separator(); 

										// --- 【右侧区域】：图片预览 (垂直居中) ---
										ui.vertical_centered(|ui| {
											// 稍微往下带一点点偏置，让它在 120px 的卡片里视觉居中
											ui.add_space(12.0); 

											let uri = format!("bytes://thumb_{}", item.id);
											let img = egui::Image::from_bytes(uri, item.image_data.clone())
												.fit_to_exact_size(egui::vec2(80.0, 80.0))
												.corner_radius(4.0)
												.sense(egui::Sense::click());

											let res = ui.add(img);
											if res.clicked() {
												self.preview_image = Some((item.time.clone(), item.image_data.clone()));
											}
											res.on_hover_text("点击放大");
										});
									});
								});

						});
					}
				});
			});
	}
}

