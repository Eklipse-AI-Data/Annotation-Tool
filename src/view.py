import tkinter as tk
from tkinter import ttk
from PIL import Image, ImageTk
from src.utils import denormalize_box
from src.ui_components import (DarkButton, DarkLabel, DarkListbox, DarkFrame, SectionLabel, 
                               SidebarFrame, THEME, DarkEntry, DarkProgressBar, ScrollableFrame, 
                               update_ui_theme, TabbedFrame, CollapsibleFrame)

class AnnotationView:
    def __init__(self, root, model):
        self.root = root
        self.model = model
        
        self.root.title("Annotation Tool - Midnight Glass")
        self.root.geometry("1400x800")
        self.root.configure(bg=THEME['bg_main'])
        
        # Rendering state
        self.scale = 1.0
        self.offset_x = 0
        self.offset_y = 0
        self.cached_dims = None
        self.cached_image_obj = None
        self.tk_image = None
        
        self.setup_ui()

    def setup_ui(self):
        # Toolbar (Top)
        self.toolbar = DarkFrame(self.root, height=30)
        self.toolbar.pack(fill=tk.X, side=tk.TOP)
        
        self.toggle_left_btn = DarkButton(self.toolbar, text="Toggle Project")
        self.toggle_left_btn.pack(side=tk.LEFT, padx=5, pady=2)
        
        self.toggle_sidebar_btn = DarkButton(self.toolbar, text="Toggle Box List")
        self.toggle_sidebar_btn.pack(side=tk.RIGHT, padx=5, pady=2)
        
        # Main Layout
        self.main_container = tk.PanedWindow(self.root, orient=tk.HORIZONTAL, bg=THEME['bg_main'], sashwidth=4, sashrelief=tk.FLAT)
        self.main_container.pack(fill=tk.BOTH, expand=True)
        
        # Left Sidebar
        self.sidebar = SidebarFrame(self.main_container, width=280)
        self.main_container.add(self.sidebar, minsize=200)
        
        # Canvas Area
        self.canvas_frame = DarkFrame(self.main_container)
        self.main_container.add(self.canvas_frame, minsize=400, stretch="always")
        
        # Right Sidebar
        self.right_sidebar = SidebarFrame(self.main_container, width=250)
        self.main_container.add(self.right_sidebar, minsize=200)
        
        self.setup_left_sidebar()
        self.setup_canvas()
        self.setup_right_sidebar()
        self.setup_status_bar()

    def setup_status_bar(self):
        self.status_bar = DarkFrame(self.root, height=25)
        self.status_bar.pack(fill=tk.X, side=tk.BOTTOM)
        
        self.status_label = DarkLabel(self.status_bar, text="Ready", font=("Segoe UI", 9))
        self.status_label.pack(side=tk.LEFT, padx=10)
        
        self.coords_label = DarkLabel(self.status_bar, text="", font=("Segoe UI", 9))
        self.coords_label.pack(side=tk.RIGHT, padx=10)

    def setup_left_sidebar(self):
        # Create Tabbed Sidebar
        self.left_tabs = TabbedFrame(self.sidebar)
        self.left_tabs.pack(fill=tk.BOTH, expand=True)
        
        # === TAB 1: PROJECT ===
        project_tab = self.left_tabs.add_tab("Project")
        
        # Project Details (Collapsible)
        proj_section = CollapsibleFrame(project_tab, "DIRECTORY")
        proj_section.pack(fill=tk.X, padx=5, pady=5)
        
        btn_frame = DarkFrame(proj_section.content, bg=THEME['bg_sidebar'])
        btn_frame.pack(fill=tk.X, padx=10, pady=5)
        
        self.open_images_btn = DarkButton(btn_frame, text="Open Images")
        self.open_images_btn.pack(fill=tk.X, pady=2)
        self.set_output_btn = DarkButton(btn_frame, text="Set Output Dir")
        self.set_output_btn.pack(fill=tk.X, pady=2)
        
        self.dir_label = DarkLabel(proj_section.content, text="No directory selected", bg=THEME['bg_sidebar'], fg=THEME['fg_text'], wraplength=230)
        self.dir_label.pack(fill=tk.X, padx=10, pady=5)

        # File List (Collapsible)
        file_section = CollapsibleFrame(project_tab, "FILES")
        file_section.pack(fill=tk.BOTH, expand=True, padx=5, pady=5)
        
        filter_frame = DarkFrame(file_section.content, bg=THEME['bg_sidebar'])
        filter_frame.pack(fill=tk.X, padx=10, pady=5)
        
        self.filter_combo = ttk.Combobox(filter_frame, state="readonly")
        self.filter_combo.pack(fill=tk.X, pady=2)
        
        btn_filter_frame = DarkFrame(filter_frame, bg=THEME['bg_sidebar'])
        btn_filter_frame.pack(fill=tk.X, pady=2)
        self.filter_btn = DarkButton(btn_filter_frame, text="Filter", width=8)
        self.filter_btn.pack(side=tk.LEFT, padx=(0, 2), expand=True, fill=tk.X)
        self.clear_filter_btn = DarkButton(btn_filter_frame, text="Clear", width=8)
        self.clear_filter_btn.pack(side=tk.RIGHT, padx=(2, 0), expand=True, fill=tk.X)
        
        self.unannotated_check = tk.Checkbutton(file_section.content, text="Show Unannotated Only (U)", 
                                               variable=self.model.unannotated_filter_active, 
                                               bg=THEME['bg_sidebar'], fg=THEME['button_highlight'], selectcolor=THEME['bg_sidebar'], 
                                               activebackground=THEME['bg_sidebar'], activeforeground=THEME['fg_highlight'])
        self.unannotated_check.pack(anchor='w', padx=10, pady=2)
        
        file_list_container = DarkFrame(file_section.content, bg=THEME['bg_sidebar'])
        file_list_container.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)
        self.file_listbox = DarkListbox(file_list_container, height=15)
        self.file_listbox.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        file_scrollbar = tk.Scrollbar(file_list_container, orient=tk.VERTICAL, command=self.file_listbox.yview)
        file_scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        self.file_listbox.configure(yscrollcommand=file_scrollbar.set)
        
        # Progress Bar fixed at bottom of project tab
        SectionLabel(project_tab, text="Progress").pack(fill=tk.X, padx=10, pady=(10, 0))
        self.progress_bar = DarkProgressBar(project_tab, height=22)
        self.progress_bar.pack(fill=tk.X, padx=10, pady=5)
        
        # === TAB 2: CLASSES ===
        classes_tab = self.left_tabs.add_tab("Classes")
        
        # Class Selection (Collapsible)
        class_section = CollapsibleFrame(classes_tab, "CLASS LIST")
        class_section.pack(fill=tk.BOTH, expand=True, padx=5, pady=5)
        
        self.create_template_btn = DarkButton(class_section.content, text="Create Template")
        self.create_template_btn.pack(fill=tk.X, padx=10, pady=2)
        
        self.class_search_var = tk.StringVar()
        search_frame = DarkFrame(class_section.content)
        search_frame.pack(fill=tk.X, padx=10, pady=(5, 0))
        self.class_search_entry = DarkEntry(search_frame, textvariable=self.class_search_var)
        self.class_search_entry.pack(fill=tk.X)
        
        self.class_listbox = DarkListbox(class_section.content, height=15)
        self.class_listbox.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)
        
        # Tools (Collapsible)
        tools_section = CollapsibleFrame(classes_tab, "TOOLS")
        tools_section.pack(fill=tk.X, padx=5, pady=5)
        
        self.copy_btn = DarkButton(tools_section.content, text="Copy Boxes (Ctrl+C)")
        self.copy_btn.pack(fill=tk.X, padx=10, pady=2)
        self.paste_btn = DarkButton(tools_section.content, text="Paste Boxes (Ctrl+V)")
        self.paste_btn.pack(fill=tk.X, padx=10, pady=2)
        
        self.auto_save_check = tk.Checkbutton(tools_section.content, text="Auto Save", variable=self.model.auto_save, 
                                             bg=THEME['bg_sidebar'], fg=THEME['fg_text'], selectcolor=THEME['bg_sidebar'], 
                                             activebackground=THEME['bg_sidebar'], activeforeground=THEME['fg_highlight'])
        self.auto_save_check.pack(anchor='w', padx=10, pady=2)
        
        self.show_labels_check = tk.Checkbutton(tools_section.content, text="Show Labels", variable=self.model.show_labels,
                                               bg=THEME['bg_sidebar'], fg=THEME['fg_text'], selectcolor=THEME['bg_sidebar'], 
                                               activebackground=THEME['bg_sidebar'], activeforeground=THEME['fg_highlight'])
        self.show_labels_check.pack(anchor='w', padx=10, pady=2)
        
        self.settings_btn = DarkButton(classes_tab, text="Settings")
        self.settings_btn.pack(fill=tk.X, padx=10, pady=10, side=tk.BOTTOM)

    def setup_canvas(self):
        self.canvas_frame.grid_rowconfigure(0, weight=1)
        self.canvas_frame.grid_columnconfigure(0, weight=1)
        self.canvas = tk.Canvas(self.canvas_frame, bg=THEME['bg_main'], highlightthickness=0)
        self.canvas.grid(row=0, column=0, sticky="nsew")
        
        self.v_scroll = tk.Scrollbar(self.canvas_frame, orient=tk.VERTICAL, command=self.canvas.yview)
        self.v_scroll.grid(row=0, column=1, sticky="ns")
        self.h_scroll = tk.Scrollbar(self.canvas_frame, orient=tk.HORIZONTAL, command=self.canvas.xview)
        self.h_scroll.grid(row=1, column=0, sticky="ew")
        self.canvas.configure(yscrollcommand=self.v_scroll.set, xscrollcommand=self.h_scroll.set)

    def setup_right_sidebar(self):
        # Create scrollable container for right sidebar
        self.right_scroll = ScrollableFrame(self.right_sidebar, bg=THEME['bg_sidebar'])
        self.right_scroll.pack(fill=tk.BOTH, expand=True)
        self.right_content = self.right_scroll.scrollable_content
        
        SectionLabel(self.right_content, text="Box Labels").pack(fill=tk.X, padx=10, pady=(10, 0))
        self.box_listbox = DarkListbox(self.right_content, selectmode=tk.EXTENDED)
        self.box_listbox.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)

    def update_dir_label(self, path):
        self.dir_label.config(text=path if path else "No directory selected")

    def toggle_right_sidebar(self, show):
        if show:
            self.main_container.add(self.right_sidebar, minsize=200)
        else:
            self.main_container.remove(self.right_sidebar)

    def toggle_left_sidebar(self, show):
        if show:
            self.main_container.add(self.sidebar, minsize=200, before=self.canvas_frame)
        else:
            self.main_container.remove(self.sidebar)

    def draw_crosshair(self, x, y):
        self.canvas.delete("crosshair")
        if x < 0 or y < 0: return
        
        # Draw lines spanning the whole canvas
        cw = self.canvas.winfo_width()
        ch = self.canvas.winfo_height()
        
        self.canvas.create_line(0, y, cw, y, fill="#FFFFFF", dash=(2, 2), tags="crosshair")
        self.canvas.create_line(x, 0, x, ch, fill="#FFFFFF", dash=(2, 2), tags="crosshair")

    def update_status(self, text):
        self.status_label.config(text=text)

    def redraw_canvas(self, fast=False):
        if not self.model.current_image_pil:
            self.canvas.delete("all")
            return
            
        # Calculate ideal dimensions
        cw = self.canvas.winfo_width()
        ch = self.canvas.winfo_height()
        iw, ih = self.model.current_image_pil.size
        
        if cw <= 1 or ch <= 1: return # Not ready yet
        
        scale_w = cw / iw
        scale_h = ch / ih
        base_scale = min(scale_w, scale_h)
        
        self.scale = base_scale * self.model.zoom_factor
        
        nw = int(iw * self.scale)
        nh = int(ih * self.scale)
        
        # Update Scrollregion
        self.canvas.configure(scrollregion=(0, 0, nw, nh))
        
        # Center image if smaller than canvas
        if nw < cw:
            self.offset_x = (cw - nw) // 2
        else:
            self.offset_x = 0
            
        if nh < ch:
            self.offset_y = (ch - nh) // 2
        else:
            self.offset_y = 0
        
        # OPTIMIZATION: Check if we need to resize the image
        if self.cached_dims != (nw, nh) or self.cached_image_obj is None:
            # We need to resize
            # Fast mode uses bilinear, otherwise Lanczos
            resample = Image.BILINEAR if fast else Image.LANCZOS
            resized = self.model.current_image_pil.resize((nw, nh), resample)
            self.tk_image = ImageTk.PhotoImage(resized)
            self.cached_image_obj = self.tk_image
            self.cached_dims = (nw, nh)
            
            # Recreate image item
            self.canvas.delete("image_bg")
            self.canvas.create_image(self.offset_x, self.offset_y, anchor=tk.NW, image=self.tk_image, tags="image_bg")
            self.canvas.tag_lower("image_bg")
        else:
            # Dimensions match, just update position
            if not self.canvas.find_withtag("image_bg"):
                 self.canvas.create_image(self.offset_x, self.offset_y, anchor=tk.NW, image=self.cached_image_obj, tags="image_bg")
                 self.canvas.tag_lower("image_bg")
            else:
                 self.canvas.coords("image_bg", self.offset_x, self.offset_y)

        # Clear overlays
        self.canvas.delete("box")
        self.canvas.delete("label")
        self.canvas.delete("handle")
        
        # Draw boxes
        for i, box in enumerate(self.model.boxes):
            is_selected = i in self.model.selected_indices
            self.draw_box_on_canvas(box, is_selected, i)
            
        # Update box list (sidebar)
        self.update_box_list()

    def draw_box_on_canvas(self, box, is_selected, index):
        if not self.model.current_image_pil: return
        
        iw, ih = self.model.current_image_pil.size
        x1, y1, x2, y2 = denormalize_box(box, iw, ih)
        
        # Scale to canvas
        cx1 = x1 * self.scale + self.offset_x
        cy1 = y1 * self.scale + self.offset_y
        cx2 = x2 * self.scale + self.offset_x
        cy2 = y2 * self.scale + self.offset_y
        
        class_info = next((c for c in self.model.classes if c['id'] == box['class_id']), None)
        
        if box['class_id'] == -1:
            color = "#FFFFFF"
            label_text = "Unlabeled"
        else:
            color = class_info['color'] if class_info else "#FFFFFF"
            label_text = class_info['name'] if class_info else "Unknown"
        
        width = 3 if is_selected else 2
        outline = "#FFFFFF" if is_selected else color
        
        # Draw Rect
        self.canvas.create_rectangle(cx1, cy1, cx2, cy2, outline=outline, width=width, tags=("box", f"box_{index}"))
        
        # Draw Label
        if self.model.show_labels.get():
            text_x = cx1
            text_y = cy1 - 15
            if text_y < 0: text_y = cy1 + 5
            self.canvas.create_text(text_x, text_y, text=label_text, fill=color, anchor=tk.SW, font=("Segoe UI", 9, "bold"), tags="label")

        # Draw handles if selected
        if is_selected:
            h_size = 6
            handles = [
                (cx1, cy1, "nw"), (cx2, cy1, "ne"),
                (cx1, cy2, "sw"), (cx2, cy2, "se")
            ]
            for hx, hy, name in handles:
                self.canvas.create_rectangle(hx-h_size/2, hy-h_size/2, hx+h_size/2, hy+h_size/2, 
                                             fill="white", outline="black", tags=("handle", f"handle_{index}_{name}"))

    def update_box_list(self):
        self.box_listbox.delete(0, tk.END)
        for i, box in enumerate(self.model.boxes):
            class_info = next((c for c in self.model.classes if c['id'] == box['class_id']), None)
            name = class_info['name'] if class_info else "Unknown"
            self.box_listbox.insert(tk.END, f"{i}: {name}")
            if i in self.model.selected_indices:
                self.box_listbox.selection_set(i)

    def apply_theme(self, theme_dict):
        """Update the entire UI with a new theme."""
        update_ui_theme(theme_dict)
        
        # Update colors recursively for self.root and all children
        self._apply_theme_to_widget(self.root)
        
        # Special case for canvas background and redraw
        self.canvas.configure(bg=THEME['bg_main'])
        self.main_container.configure(bg=THEME['bg_main'])
        
        # Redraw everything
        self.redraw_canvas()

    def _apply_theme_to_widget(self, widget):
        """Recursively apply theme to widgets."""
        from src.ui_components import DarkButton, DarkLabel, DarkListbox, DarkFrame, SectionLabel, SidebarFrame, DarkEntry, DarkProgressBar, ScrollableFrame
        
        try:
            if isinstance(widget, DarkFrame):
                widget.configure(bg=THEME['bg_main'])
            elif isinstance(widget, SidebarFrame):
                widget.configure(bg=THEME['bg_sidebar'])
            elif isinstance(widget, ScrollableFrame):
                widget.configure(bg=THEME['bg_sidebar'])
                widget.canvas.configure(bg=THEME['bg_sidebar'])
                widget.scrollable_content.configure(bg=THEME['bg_sidebar'])
            elif isinstance(widget, TabbedFrame):
                widget.configure(bg=THEME['bg_sidebar'])
                widget.header.configure(bg=THEME['bg_sidebar'])
                widget.content.configure(bg=THEME['bg_sidebar'])
                for btn, frame in widget.tabs.values():
                    btn.configure(bg=THEME['button_bg'], fg=THEME['button_fg'], activebackground=THEME['button_highlight'])
                    frame.configure(bg=THEME['bg_sidebar'])
            elif isinstance(widget, CollapsibleFrame):
                widget.configure(bg=THEME['bg_sidebar'])
                widget.header.configure(bg=THEME['bg_sidebar'], fg=THEME['button_highlight'], activebackground=THEME['bg_sidebar'])
                widget.content.configure(bg=THEME['bg_sidebar'])
            elif isinstance(widget, SectionLabel):
                widget.configure(bg=THEME['bg_sidebar'], fg=THEME['button_highlight'])
            elif isinstance(widget, DarkLabel):
                # Labels might be on sidebar or main
                bg = widget.master.cget('bg') if hasattr(widget.master, 'cget') else THEME['bg_main']
                widget.configure(bg=bg, fg=THEME['fg_text'])
            elif isinstance(widget, DarkButton):
                widget.configure(bg=THEME['button_bg'], fg=THEME['button_fg'], activebackground=THEME['button_highlight'])
            elif isinstance(widget, DarkEntry):
                widget.configure(bg=THEME['entry_bg'], fg=THEME['entry_fg'], insertbackground=THEME['fg_text'],
                                 highlightbackground=THEME['border'], highlightcolor=THEME['accent'])
            elif isinstance(widget, DarkListbox):
                widget.configure(bg=THEME['list_bg'], fg=THEME['entry_fg'], 
                                 selectbackground=THEME['selection'], selectforeground=THEME['fg_highlight'])
            elif isinstance(widget, DarkProgressBar):
                widget.bg_color = THEME['entry_bg']
                widget.fg_color = THEME['button_highlight']
                widget.text_color = THEME['fg_text']
                widget.configure(bg=THEME['entry_bg'], highlightbackground=THEME['border'])
                widget._draw()
            elif isinstance(widget, tk.Checkbutton):
                bg = widget.master.cget('bg') if hasattr(widget.master, 'cget') else THEME['bg_sidebar']
                widget.configure(bg=bg, fg=THEME['fg_text'], selectcolor=bg, activebackground=bg)
            elif isinstance(widget, tk.PanedWindow):
                widget.configure(bg=THEME['bg_main'])
            elif isinstance(widget, tk.Scrollbar):
                pass # Themed scrollbars are tricky without ttk styles
        except Exception as e:
            # print(f"Error updating widget {widget}: {e}")
            pass

        for child in widget.winfo_children():
            self._apply_theme_to_widget(child)
