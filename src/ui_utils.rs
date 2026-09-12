use eframe::egui;

use crate::OCRTApp;


#[derive(Debug, PartialEq, Copy, Clone)]
pub enum TitleBarButton {
    Close,
    Maximize,
    Minimize,
}
impl TitleBarButton {
    // 方便循环遍历
    pub const ALL: [Self; 3] = [Self::Close, Self::Maximize, Self::Minimize];
}

//标题栏
impl OCRTApp {
	pub fn render_custom_title_bar(&mut self, ctx: &egui::Context) {
		let title_bar_color = egui::Color32::from_rgb(130, 109, 166);
		let border_color = egui::Color32::from_rgb(83, 69, 106);

		egui::TopBottomPanel::top("title_bar")
			.frame(egui::Frame::NONE.fill(title_bar_color))
			.exact_height(30.0)
			.show(ctx, |ui| {
				let title_rect = ui.max_rect();

				// 绘制底边框线
				ui.painter().hline(
					title_rect.x_range(), 
					title_rect.bottom(), 
					(1.0, border_color)
				);

				// --- A. 定义可拖拽区域 ---
				let button_zone_width = 135.0;
				let drag_rect = egui::Rect::from_min_max(
					title_rect.min,
					egui::pos2(title_rect.right() - button_zone_width, title_rect.bottom())
				);

				//// --- B. 窗口拖拽逻辑 ---
				let drag_id = ui.id().with("drag_area");
				let pointer_interact = ui.interact(drag_rect, drag_id, egui::Sense::drag());
				if pointer_interact.drag_started_by(egui::PointerButton::Primary) {
					ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
				}
				if ctx.input(|i| i.pointer.any_released()) {
					ctx.request_repaint();
					ui.interact(egui::Rect::ZERO, egui::Id::NULL, egui::Sense::click());
				}

				// --- C. UI 内容渲染 ---
				ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
					ui.add_space(10.0);

					let draw_dead_text = |ui: &mut egui::Ui, text: String, color: egui::Color32, strong: bool| {
						let font_id = if strong {
							egui::FontId::proportional(14.0) // 对应 strong
						} else {
							egui::FontId::proportional(13.0)
						};

						// 1. 预留空间
						let galley = ui.painter().layout_no_wrap(text, font_id, color);
						let (rect, _) = ui.allocate_at_least(galley.size(), egui::Sense::hover());

						// 2. 绘制（直接画在 Painter 上，没有任何交互逻辑）
						ui.painter().galley(rect.min, galley, color);
					};

					// 程序名
					ui.add_space(10.0);
					draw_dead_text(ui, self.app_title.clone(), egui::Color32::WHITE, true);




					// 右侧按钮组（最小化、最大化、关闭等）
					ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
						ui.spacing_mut().item_spacing.x = 0.0;
						self.draw_custom_buttons(ui, ctx);
					});
				});

				// --- D. 双击最大化逻辑 ---
				let drag_res = ui.interact(
					drag_rect, 
					ui.id().with("drag_area"), 
					egui::Sense::click()
				);

				if drag_res.double_clicked() {
					let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
					ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
				}
			});
	}
}
//最大化最小化还原按钮
impl OCRTApp {
	pub fn draw_custom_buttons(&self, ui: &mut egui::Ui, ctx: &egui::Context) {
		let stroke = egui::Stroke::new(2.0, egui::Color32::WHITE);
		let stroke_fine = egui::Stroke::new(1.0, egui::Color32::WHITE);
		let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
		let pointer_pos = ctx.input(|i| i.pointer.interact_pos());

		for button in TitleBarButton::ALL {
			let (rect, res) = ui.allocate_at_least(egui::vec2(44.0, 30.0), egui::Sense::click());
			let is_hovered = pointer_pos.map_or(false, |pos| rect.contains(pos));

			if is_hovered {
				ctx.request_repaint();
				let bg_color = match button {
					TitleBarButton::Close => egui::Color32::from_rgb(232, 17, 35), // 红色
					_ => egui::Color32::from_white_alpha(30),                    // 浅灰色
				};
				ui.painter().rect_filled(rect, 0.0, bg_color);
			}

			// 绘制图标与处理点击
			match button {
				TitleBarButton::Close => {
					let c = rect.center();
					ui.painter().line_segment([c - egui::vec2(5.0, 5.0), c + egui::vec2(5.0, 5.0)], stroke);
					ui.painter().line_segment([c - egui::vec2(-5.0, 5.0), c + egui::vec2(-5.0, 5.0)], stroke);
					if res.clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
				}
				TitleBarButton::Maximize => {
					if is_maximized {
						// 还原图标
						let shift = egui::vec2(1.0, -1.0);
						let size = egui::vec2(10.0, 10.0);
						let center = rect.center();
						ui.painter().rect_stroke(egui::Rect::from_center_size(center + shift, size), 0.0, stroke_fine, egui::StrokeKind::Inside);

						let rect2 = egui::Rect::from_center_size(center - shift, size);
						ui.painter().rect_filled(rect2, 0.0, egui::Color32::from_rgb(130, 109, 166)); 
						ui.painter().rect_stroke(rect2, 0.0, stroke_fine, egui::StrokeKind::Inside);
					} else {
						// 最大化图标
						ui.painter().rect_stroke(rect.shrink2(egui::vec2(16.0, 9.0)), 0.0, stroke_fine, egui::StrokeKind::Inside);
					}
					if res.clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized)); }
				}
				TitleBarButton::Minimize => {
					ui.painter().hline(rect.center().x - 5.0..=rect.center().x + 5.0, rect.center().y, stroke);
					if res.clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)); }
				}
			}
		}
	}
}
//边框
impl OCRTApp {
	pub fn draw_window_frame(&self, ctx: &egui::Context) {
		let border_color = egui::Color32::from_rgb(83, 69, 106);
		let rect = ctx.input(|i| i.viewport_rect());
		let painter = ctx.layer_painter(egui::LayerId::debug());

		// 注意：这里用 2.0 宽度可能会稍微遮挡内部像素，如果觉得粗可以改回 1.0
		painter.line_segment([rect.left_bottom(), rect.right_bottom()], (2.0, border_color));
		painter.line_segment([rect.left_top(), rect.left_bottom()], (2.0, border_color));
		painter.line_segment([rect.right_top(), rect.right_bottom()], (2.0, border_color));
	}
}

