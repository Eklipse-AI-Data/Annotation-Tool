import os
import shutil
import threading
import concurrent.futures
import tkinter as tk
from tkinter import filedialog, messagebox, simpledialog, ttk
from PIL import Image
from src.utils import (natural_sort_key, save_config, save_classes, load_classes,
                       create_class_mapping, update_annotation_file, backup_annotations,
                       normalize_box, denormalize_box)
from src.ui_components import DarkFrame, DarkLabel, DarkButton, DarkEntry, THEME

class AnnotationController:
    def __init__(self, model, view):
        self.model = model
        self.view = view
        
        self.bind_events()
        self.init_state()
        self.redraw_timer = None

    def init_state(self):
        # Update UI with initial model state
        self.view.update_dir_label(self.model.image_dir)
        self.update_class_list()
        self.update_filter_combo()
        
        if self.model.image_dir:
            self.load_images()
            if self.model.image_list:
                index = self.model.current_image_index
                if 0 <= index < len(self.model.image_list):
                    self.load_image(index)
                else:
                    self.load_image(0)
        
        # Initial sidebar state
        if not self.model.show_right_sidebar.get():
            self.view.toggle_right_sidebar(False)
            
        if not self.model.show_left_sidebar.get():
            self.view.toggle_left_sidebar(False)
            
        # Apply theme from session
        from src.themes import PREDEFINED_THEMES
        theme_name = self.model.current_theme_name
        if theme_name in PREDEFINED_THEMES:
            self.view.apply_theme(PREDEFINED_THEMES[theme_name])

    def bind_events(self):
        # UI Button Commands
        self.view.open_images_btn.configure(command=self.select_image_dir)
        self.view.set_output_btn.configure(command=self.select_output_dir)
        self.view.toggle_left_btn.configure(command=self.toggle_left_sidebar)
        self.view.toggle_sidebar_btn.configure(command=self.toggle_right_sidebar)
        self.view.settings_btn.configure(command=self.open_settings_dialog)
        self.view.filter_btn.configure(command=self.apply_image_filter)
        self.view.clear_filter_btn.configure(command=self.clear_image_filter)
        self.view.copy_btn.configure(command=self.copy_boxes)
        self.view.paste_btn.configure(command=self.paste_boxes)
        
        # Listbox Binds
        self.view.class_listbox.bind('<<ListboxSelect>>', self.on_class_select)
        self.view.file_listbox.bind('<<ListboxSelect>>', self.on_file_select)
        
        # Canvas Binds
        self.view.canvas.bind('<Configure>', lambda e: self.view.redraw_canvas())
        self.view.canvas.bind('<Button-1>', self.on_canvas_click)
        self.view.canvas.bind('<B1-Motion>', self.on_canvas_drag)
        self.view.canvas.bind('<ButtonRelease-1>', self.on_canvas_release)
        self.view.canvas.bind('<Motion>', self.on_canvas_motion)
        
        # Panning and Zooming
        self.view.canvas.bind('<Button-3>', self.start_pan)
        self.view.canvas.bind('<B3-Motion>', self.on_pan)
        self.view.canvas.bind('<MouseWheel>', self.on_zoom)
        
        # Keyboard Shortcuts (using config)
        root = self.view.root
        cfg = self.model.config
        root.bind(cfg['prev_image'], lambda e: self.prev_image())
        root.bind(cfg['next_image'], lambda e: self.next_image())
        root.bind(cfg['delete_box'], lambda e: self.delete_selected_box())
        root.bind(cfg['copy'], lambda e: self.copy_boxes())
        root.bind(cfg['paste'], lambda e: self.paste_boxes())
        root.bind(cfg['cycle_class'], lambda e: self.cycle_class())
        root.bind(cfg['edit_class'], lambda e: self.edit_selected_box_class())
        root.bind(cfg['deselect'], lambda e: self.deselect_class())
        root.bind(cfg['undo'], lambda e: self.undo())
        root.bind(cfg['redo'], lambda e: self.redo())
        
        # Secondary Navigation
        root.bind("<Left>", lambda e: self.prev_image())
        root.bind("<Right>", lambda e: self.next_image())
        
        # Bounding Box Presets shortcuts - robust handling for various layouts and Numpad
        for i in range(1, 10):
            # Standard number keys
            root.bind(f"<Control-Key-{i}>", lambda e, num=i: self.apply_preset(num))
            root.bind(f"<Control-Shift-Key-{i}>", lambda e, num=i: self.save_preset(num))
            # Numpad compatibility
            root.bind(f"<Control-KP_{i}>", lambda e, num=i: self.apply_preset(num))
            root.bind(f"<Control-Shift-KP_{i}>", lambda e, num=i: self.save_preset(num))
            
        # Common shifted symbols for Ctrl+Shift+Number (specifically helps on Windows/Windows Layouts)
        shift_symbols = {
            "exclam": 1, "at": 2, "numbersign": 3, "dollar": 4, "percent": 5,
            "asciicircum": 6, "ampersand": 7, "asterisk": 8, "parenleft": 9
        }
        for sym, num in shift_symbols.items():
            root.bind(f"<Control-{sym}>", lambda e, n=num: self.save_preset(n))
        
        # U key for unannotated filter toggle
        root.bind("<u>", lambda e: self.toggle_unannotated_filter_key())
        
        # Search Bar Trace
        self.view.class_search_var.trace("w", self.filter_classes)
        
        # Window Close
        root.protocol("WM_DELETE_WINDOW", self.on_close)

    def select_image_dir(self):
        directory = filedialog.askdirectory()
        if directory:
            # Save current annotations before switching directories
            if self.model.current_image_index != -1 and self.model.auto_save.get():
                self.model.save_annotations()
                
            self.model.image_dir = directory
            if not self.model.output_dir:
                self.model.output_dir = directory
            self.view.update_dir_label(directory)
            self.load_images()
            self.load_image(0)

    def select_output_dir(self):
        directory = filedialog.askdirectory()
        if directory:
            self.model.output_dir = directory
            # Reload annotations for current image if one is loaded
            if self.model.image_list and self.model.current_image_index != -1:
                filename = self.model.image_list[self.model.current_image_index]
                self.model.load_annotations(filename)
                self.view.redraw_canvas()
            
            self.update_progress_bar()
            messagebox.showinfo("Info", f"Output directory set to: {directory}")

    def load_images(self):
        if not self.model.image_dir: return
        files = [f for f in os.listdir(self.model.image_dir) 
                 if f.lower().endswith(('.png', '.jpg', '.jpeg'))]
        self.model.full_image_list = sorted(files, key=natural_sort_key)
        self.model.image_list = list(self.model.full_image_list)
        
        self.view.file_listbox.delete(0, tk.END)
        for f in self.model.image_list:
            self.view.file_listbox.insert(tk.END, f)
        
        self.update_progress_bar()

    def load_image(self, index):
        if not self.model.image_list: return
        if 0 <= index < len(self.model.image_list):
            # Auto save previous image annotations
            if self.model.current_image_index != -1 and self.model.auto_save.get():
                self.model.save_annotations()

            self.model.current_image_index = index
            filename = self.model.image_list[index]
            path = os.path.join(self.model.image_dir, filename)
            
            try:
                self.model.current_image_pil = Image.open(path)
                # Reset view cache
                self.view.cached_dims = None
                self.view.cached_image_obj = None
                
                # Load annotations for new image
                self.model.load_annotations(filename)
                
                # Update title
                self.view.root.title(f"Annotation Tool - {filename}")
                
                # Redraw
                self.view.redraw_canvas()
                
            except Exception as e:
                print(f"Error loading image: {e}")
            
            # Update listbox selection
            self.view.file_listbox.selection_clear(0, tk.END)
            self.view.file_listbox.selection_set(index)
            self.view.file_listbox.see(index)

    def prev_image(self):
        if self.model.current_image_index > 0:
            self.load_image(self.model.current_image_index - 1)

    def next_image(self):
        if self.model.current_image_index < len(self.model.image_list) - 1:
            self.load_image(self.model.current_image_index + 1)

    def toggle_right_sidebar(self):
        current = self.model.show_right_sidebar.get()
        self.model.show_right_sidebar.set(not current)
        self.view.toggle_right_sidebar(not current)

    def toggle_left_sidebar(self):
        current = self.model.show_left_sidebar.get()
        self.model.show_left_sidebar.set(not current)
        self.view.toggle_left_sidebar(not current)

    def cycle_class(self):
        if not self.model.classes: return
        self.model.current_class_index = (self.model.current_class_index + 1) % len(self.model.classes)
        self.update_class_list()

    def edit_selected_box_class(self):
        if self._is_input_focused(): return
        if not self.model.selected_indices: return
        
        top = tk.Toplevel(self.view.root)
        top.title("Select Class")
        top.geometry("300x400")
        top.configure(bg=THEME['bg_main'])
        
        DarkLabel(top, text="SELECT CLASS", font=("Segoe UI", 10, "bold")).pack(pady=10)
        
        lb_frame = DarkFrame(top)
        lb_frame.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)
        
        lb = tk.Listbox(lb_frame, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                         selectbackground=THEME['selection'], relief=tk.FLAT, bd=0)
        lb.pack(fill=tk.BOTH, expand=True)
        
        for c in self.model.classes:
            lb.insert(tk.END, f"  {c['name']}")
            
        def on_select(event):
            sel = lb.curselection()
            if sel:
                self.model.save_state()
                new_class_id = self.model.classes[sel[0]]['id']
                for idx in self.model.selected_indices:
                    self.model.boxes[idx]['class_id'] = new_class_id
                self.view.redraw_canvas()
                top.destroy()
                
        lb.bind('<<ListboxSelect>>', on_select)

    def deselect_class(self):
        self.model.current_class_index = -1
        self.update_class_list()

    def select_class_by_number(self, num):
        idx = num - 1
        if 0 <= idx < len(self.model.classes):
            self.model.current_class_index = idx
            self.update_class_list()

    def toggle_unannotated_filter_key(self):
        current = self.model.unannotated_filter_active.get()
        self.model.unannotated_filter_active.set(not current)
        self.toggle_unannotated_filter()

    def toggle_unannotated_filter(self):
        if self.model.unannotated_filter_active.get():
            if not self.model.output_dir:
                messagebox.showwarning("Warning", "Please set Output Directory first.")
                self.model.unannotated_filter_active.set(False)
                return
            unannotated = []
            for img in self.model.full_image_list:
                name, _ = os.path.splitext(img)
                txt_path = os.path.join(self.model.output_dir, name + ".txt")
                if not os.path.exists(txt_path) or os.path.getsize(txt_path) == 0:
                    unannotated.append(img)
            self.model.image_list = unannotated
        else:
            self.model.image_list = list(self.model.full_image_list)
        
        self.view.file_listbox.delete(0, tk.END)
        for f in self.model.image_list:
            self.view.file_listbox.insert(tk.END, f)
        if self.model.image_list:
            self.load_image(0)
        self.update_progress_bar()

    def filter_classes(self, *args):
        search_term = self.view.class_search_var.get().lower()
        self.model.filtered_classes = [
            (i, c) for i, c in enumerate(self.model.classes)
            if search_term in c['name'].lower()
        ]
        self.update_class_list()

    def update_class_list(self):
        self.view.class_listbox.delete(0, tk.END)
        for i, (real_idx, c) in enumerate(self.model.filtered_classes):
            self.view.class_listbox.insert(tk.END, f"{c['id']}: {c['name']}")
            self.view.class_listbox.itemconfig(i, fg=c['color'])
            if real_idx == self.model.current_class_index:
                self.view.class_listbox.selection_set(i)
                self.view.class_listbox.see(i)

    def update_filter_combo(self):
        values = [f"{c['id']}: {c['name']}" for c in self.model.classes]
        self.view.filter_combo['values'] = values

    def on_class_select(self, event):
        selection = self.view.class_listbox.curselection()
        if selection:
            list_idx = selection[0]
            real_idx, _ = self.model.filtered_classes[list_idx]
            self.model.current_class_index = real_idx

    def on_file_select(self, event):
        selection = self.view.file_listbox.curselection()
        if selection:
            self.load_image(selection[0])

    def update_progress_bar(self):
        count, total = self.model.get_annotated_count()
        percent = int((count / total) * 100) if total > 0 else 0
        text = f"{count}/{total} ({percent}%)"
        self.view.progress_bar.set_value(count, total, text)

    def on_close(self):
        # Save current image if auto_save is on
        if self.model.current_image_index != -1 and self.model.auto_save.get():
            self.model.save_annotations()
            
        self.model.save_session()
        self.view.root.destroy()

    # ==================== SETTINGS DIALOG ====================

    def open_settings_dialog(self):
        top = tk.Toplevel(self.view.root)
        top.title("Settings")
        top.geometry("600x600")
        top.configure(bg=THEME['bg_main'])
        
        # Create Notebook (Tabbed Interface)
        notebook = ttk.Notebook(top)
        notebook.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        
        # Tab 1: Keybindings
        keybindings_tab = DarkFrame(notebook)
        notebook.add(keybindings_tab, text="Keybindings")
        
        # Tab 2: Class Management
        class_mgmt_tab = DarkFrame(notebook)
        notebook.add(class_mgmt_tab, text="Class Management")
        
        # Tab 3: Batch Operations
        batch_ops_tab = DarkFrame(notebook)
        notebook.add(batch_ops_tab, text="Batch Operations")
        
        # Tab 4: Game Presets
        game_presets_tab = DarkFrame(notebook)
        notebook.add(game_presets_tab, text="Game Presets")
        
        # Tab 5: Box Presets (Slots)
        box_presets_tab = DarkFrame(notebook)
        notebook.add(box_presets_tab, text="Box Presets")
        
        # Tab 6: Themes
        themes_tab = DarkFrame(notebook)
        notebook.add(themes_tab, text="Themes")
        
        # Setup Tabs
        self.setup_keybindings_tab(keybindings_tab, top)
        self.setup_class_management_tab(class_mgmt_tab)
        self.setup_batch_operations_tab(batch_ops_tab)
        self.setup_game_presets_tab(game_presets_tab)
        self.setup_box_presets_tab(box_presets_tab)
        self.setup_themes_tab(themes_tab)

    def setup_keybindings_tab(self, parent, window):
        DarkLabel(parent, text="Configure Keybindings", font=("Segoe UI", 12, "bold")).pack(pady=10)
        
        canvas = tk.Canvas(parent, bg=THEME['bg_main'], highlightthickness=0)
        scrollbar = tk.Scrollbar(parent, orient="vertical", command=canvas.yview)
        scrollable_frame = DarkFrame(canvas)
        
        scrollable_frame.bind("<Configure>", lambda e: canvas.configure(scrollregion=canvas.bbox("all")))
        canvas.create_window((0, 0), window=scrollable_frame, anchor="nw")
        canvas.configure(yscrollcommand=scrollbar.set)
        
        canvas.pack(side="left", fill="both", expand=True, padx=10)
        scrollbar.pack(side="right", fill="y")
        
        for action, key in self.model.config.items():
            row_frame = DarkFrame(scrollable_frame)
            row_frame.pack(fill=tk.X, pady=2)
            DarkLabel(row_frame, text=action.replace("_", " ").title()).pack(side=tk.LEFT, padx=5)
            btn = DarkButton(row_frame, text=key, width=15)
            btn.pack(side=tk.RIGHT, padx=5)
            btn.configure(command=lambda b=btn, a=action: self.capture_key(b, a))
            
        DarkButton(parent, text="Save & Close", command=lambda: self.save_settings(window)).pack(pady=10)

    def setup_class_management_tab(self, parent):
        DarkLabel(parent, text="Manage Classes", font=("Segoe UI", 12, "bold")).pack(pady=10)
        info_text = "Add, remove, or reorder classes. Changes will update all annotation files."
        DarkLabel(parent, text=info_text, wraplength=550).pack(pady=5)
        
        main_frame = DarkFrame(parent)
        main_frame.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        
        list_frame = DarkFrame(main_frame)
        list_frame.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=(0, 10))
        
        list_container = DarkFrame(list_frame)
        list_container.pack(fill=tk.BOTH, expand=True)
        
        self.class_mgmt_listbox = tk.Listbox(list_container, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                                            selectbackground=THEME['selection'], relief=tk.FLAT, bd=0)
        self.class_mgmt_listbox.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        
        self.temp_classes = [c['name'] for c in self.model.classes]
        self.update_class_mgmt_list()
        
        button_frame = DarkFrame(main_frame)
        button_frame.pack(side=tk.RIGHT, fill=tk.Y)
        
        self.new_class_entry = tk.Entry(button_frame, bg=THEME['entry_bg'], fg=THEME['entry_fg'], insertbackground=THEME['fg_text'])
        self.new_class_entry.pack(fill=tk.X, pady=2)
        
        DarkButton(button_frame, text="Add Class", command=self.add_class_to_temp_list).pack(fill=tk.X, pady=2)
        DarkButton(button_frame, text="Remove Selected", command=self.remove_class_from_temp_list).pack(fill=tk.X, pady=5)
        DarkButton(button_frame, text="Move Up", command=self.move_class_up).pack(fill=tk.X, pady=2)
        DarkButton(button_frame, text="Move Down", command=self.move_class_down).pack(fill=tk.X, pady=2)
        DarkButton(button_frame, text="Apply Changes", command=self.apply_class_changes, bg=THEME['accent']).pack(fill=tk.X, pady=10)

    def setup_batch_operations_tab(self, parent):
        DarkLabel(parent, text="Batch Replace Class IDs", font=("Segoe UI", 12, "bold")).pack(pady=10)
        info_text = "Replace all instances of one class ID with another in the directory."
        DarkLabel(parent, text=info_text, wraplength=550).pack(pady=5)
        
        selection_frame = DarkFrame(parent)
        selection_frame.pack(fill=tk.X, padx=10, pady=10)
        
        self.batch_old_listbox = tk.Listbox(selection_frame, height=8, bg=THEME['list_bg'], fg=THEME['fg_text'])
        self.batch_old_listbox.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=5)
        
        self.batch_new_listbox = tk.Listbox(selection_frame, height=8, bg=THEME['list_bg'], fg=THEME['fg_text'])
        self.batch_new_listbox.pack(side=tk.RIGHT, fill=tk.BOTH, expand=True, padx=5)
        
        for i, cls in enumerate(self.model.classes):
            display_text = f"{cls['id']}: {cls['name']}"
            self.batch_old_listbox.insert(tk.END, display_text)
            self.batch_new_listbox.insert(tk.END, display_text)
            
        DarkButton(parent, text="Execute Batch Replace", command=self.execute_batch_replace, bg=THEME['accent']).pack(pady=10)

    def setup_game_presets_tab(self, parent):
        DarkLabel(parent, text="Switch Game Classes", font=("Segoe UI", 12, "bold")).pack(pady=10)
        
        self.preset_listbox = tk.Listbox(parent, bg=THEME['list_bg'], fg=THEME['fg_text'], selectbackground=THEME['selection'])
        self.preset_listbox.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)
        
        self.refresh_preset_list()
        
        btn_frame = DarkFrame(parent)
        btn_frame.pack(fill=tk.X, padx=10, pady=5)
        DarkButton(btn_frame, text="Load Selected", command=self.load_selected_preset, bg=THEME['accent']).pack(side=tk.LEFT, expand=True, fill=tk.X, padx=2)
        DarkButton(btn_frame, text="Refresh", command=self.refresh_preset_list).pack(side=tk.RIGHT, expand=True, fill=tk.X, padx=2)

    def setup_box_presets_tab(self, parent):
        DarkLabel(parent, text="Manage Bounding Box Presets", font=("Segoe UI", 12, "bold")).pack(pady=10)
        info_text = "View and manage temporarily stored bounding box slots (1-9).\nThese are cleared when the application is closed."
        DarkLabel(parent, text=info_text, wraplength=550).pack(pady=5)
        
        container = DarkFrame(parent)
        container.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        
        # Use a listbox to show status of slots
        self.box_presets_lb = tk.Listbox(container, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                                         selectbackground=THEME['selection'], relief=tk.FLAT, bd=0, height=10)
        self.box_presets_lb.pack(fill=tk.BOTH, expand=True)
        
        self.update_box_presets_list()
        
        btn_frame = DarkFrame(parent)
        btn_frame.pack(fill=tk.X, padx=10, pady=10)
        
        DarkButton(btn_frame, text="Save Selected/All to Selected Slot", command=self.save_to_selected_slot, bg=THEME['accent']).pack(fill=tk.X, pady=2)
        DarkButton(btn_frame, text="Clear All Presets", command=self.clear_all_presets, bg="#882222").pack(fill=tk.X, pady=2)

    def save_to_selected_slot(self):
        sel = self.box_presets_lb.curselection()
        if not sel:
            messagebox.showwarning("Warning", "Please select a slot in the list first.")
            return
        slot = sel[0] + 1
        self.save_preset(slot)
        self.update_box_presets_list()

    def update_box_presets_list(self):
        self.box_presets_lb.delete(0, tk.END)
        for i in range(1, 10):
            boxes = self.model.presets.get(i, [])
            status = f"Slot {i}: {len(boxes)} box(es)" if boxes else f"Slot {i}: Empty"
            self.box_presets_lb.insert(tk.END, f"  {status}")

    def clear_all_presets(self):
        if messagebox.askyesno("Confirm", "Clear all temporarily stored box presets?"):
            self.model.presets.clear()
            self.update_box_presets_list()
            self.view.update_status("All box presets cleared")

    def save_settings(self, window):
        save_config("config.json", self.model.config)
        self.bind_events()
        window.destroy()
        messagebox.showinfo("Settings", "Settings saved!")

    def capture_key(self, button, action):
        cap = tk.Toplevel(self.view.root)
        cap.title("Press Any Key")
        cap.geometry("250x100")
        cap.configure(bg=THEME['bg_main'])
        DarkLabel(cap, text=f"Capturing key for:\n{action.replace('_', ' ').title()}").pack(expand=True)
        
        def on_key(event):
            keysym = event.keysym
            if keysym in ('Control_L', 'Control_R', 'Shift_L', 'Shift_R', 'Alt_L', 'Alt_R', 'Caps_Lock'): return
            
            parts = []
            if event.state & 0x0004: parts.append("Control")
            if event.state & 131072: parts.append("Alt")
            if event.state & 0x0001: parts.append("Shift")
            
            if len(keysym) > 1:
                key_part = f"<{keysym}>"
            else:
                key_part = keysym.lower()
            
            if parts:
                if key_part.startswith("<") and key_part.endswith(">"):
                    key_part = key_part[1:-1]
                new_key = f"<{'-'.join(parts)}-{key_part}>"
            else:
                new_key = f"<{keysym}>"
                
            self.model.config[action] = new_key
            button.config(text=new_key)
            cap.destroy()
            
        cap.bind("<Key>", on_key)
        cap.focus_set()

    def update_class_mgmt_list(self):
        self.class_mgmt_listbox.delete(0, tk.END)
        for name in self.temp_classes:
            self.class_mgmt_listbox.insert(tk.END, name)

    def add_class_to_temp_list(self):
        name = self.new_class_entry.get().strip()
        if name and name not in self.temp_classes:
            self.temp_classes.append(name)
            self.update_class_mgmt_list()
            self.new_class_entry.delete(0, tk.END)

    def remove_class_from_temp_list(self):
        sel = self.class_mgmt_listbox.curselection()
        if sel:
            self.temp_classes.pop(sel[0])
            self.update_class_mgmt_list()

    def move_class_up(self):
        sel = self.class_mgmt_listbox.curselection()
        if sel and sel[0] > 0:
            idx = sel[0]
            self.temp_classes[idx], self.temp_classes[idx-1] = self.temp_classes[idx-1], self.temp_classes[idx]
            self.update_class_mgmt_list()
            self.class_mgmt_listbox.selection_set(idx-1)

    def move_class_down(self):
        sel = self.class_mgmt_listbox.curselection()
        if sel and sel[0] < len(self.temp_classes)-1:
            idx = sel[0]
            self.temp_classes[idx], self.temp_classes[idx+1] = self.temp_classes[idx+1], self.temp_classes[idx]
            self.update_class_mgmt_list()
            self.class_mgmt_listbox.selection_set(idx+1)

    def apply_class_changes(self):
        if not self.temp_classes: return
        old_classes = [c['name'] for c in self.model.classes]
        if old_classes == self.temp_classes: return
        
        if not messagebox.askyesno("Confirm", "Apply class changes and update all annotation files?"): return
        
        class_mapping = create_class_mapping(old_classes, self.temp_classes)
        
        if self.model.image_dir:
            for f in os.listdir(self.model.image_dir):
                if f.lower().endswith('.txt'):
                    update_annotation_file(os.path.join(self.model.image_dir, f), class_mapping)
        
        save_classes("data/predefined_classes.txt", self.temp_classes)
        self.model.classes = load_classes("data/predefined_classes.txt")
        self.update_class_list()
        self.update_filter_combo()
        if self.model.current_image_index != -1:
            self.load_image(self.model.current_image_index)
        messagebox.showinfo("Success", "Classes updated!")

    def execute_batch_replace(self):
        old_sel = self.batch_old_listbox.curselection()
        new_sel = self.batch_new_listbox.curselection()
        if not old_sel or not new_sel: return
        
        old_id = self.model.classes[old_sel[0]]['id']
        new_id = self.model.classes[new_sel[0]]['id']
        
        if not messagebox.askyesno("Confirm", f"Replace all ID {old_id} with {new_id}?"): return
        
        mapping = {old_id: new_id}
        if self.model.image_dir:
            for f in os.listdir(self.model.image_dir):
                if f.lower().endswith('.txt'):
                    update_annotation_file(os.path.join(self.model.image_dir, f), mapping)
        
        if self.model.current_image_index != -1:
            self.load_image(self.model.current_image_index)
        messagebox.showinfo("Success", "Batch replace complete!")

    def refresh_preset_list(self):
        self.preset_listbox.delete(0, tk.END)
        if os.path.exists("data"):
            presets = [f for f in os.listdir("data") if f.endswith('.txt') and f != 'predefined_classes.txt']
            for p in sorted(presets):
                self.preset_listbox.insert(tk.END, p)

    def load_selected_preset(self):
        sel = self.preset_listbox.curselection()
        if not sel: return
        
        preset = self.preset_listbox.get(sel[0])
        if messagebox.askyesno("Confirm", f"Overwrite current classes with {preset}?"):
            shutil.copy2(os.path.join("data", preset), "data/predefined_classes.txt")
            self.model.classes = load_classes("data/predefined_classes.txt")
            self.update_class_list()
            self.update_filter_combo()
            if self.model.current_image_index != -1:
                self.load_image(self.model.current_image_index)
            messagebox.showinfo("Success", "Preset loaded!")

    def on_canvas_click(self, event):
        self.view.canvas.focus_set()
        if not self.model.current_image_pil: return
        x = self.view.canvas.canvasx(event.x)
        y = self.view.canvas.canvasy(event.y)
        
        iw, ih = self.model.current_image_pil.size
        # Clamp to image for math-heavy operations
        cx = max(self.view.offset_x, min(self.view.offset_x + (iw * self.view.scale), x))
        cy = max(self.view.offset_y, min(self.view.offset_y + (ih * self.view.scale), y))

        # 1. Check handles (priority)
        box_idx, handle_name = self.find_handle_at(x, y)
        if box_idx != -1 and box_idx in self.model.selected_indices:
            self.model.resize_mode = True
            self.model.active_handle_index = handle_name
            self.model.active_box_index = box_idx
            self.model.start_x, self.model.start_y = cx, cy
            return

        # 2. Check for box selection
        clicked_box_idx = self.find_box_at(x, y)
        if clicked_box_idx != -1:
            if event.state & 0x0001:  # Shift
                if clicked_box_idx in self.model.selected_indices:
                    self.model.selected_indices.remove(clicked_box_idx)
                else:
                    self.model.selected_indices.add(clicked_box_idx)
            else:
                if clicked_box_idx not in self.model.selected_indices:
                    self.model.selected_indices = {clicked_box_idx}
            
            self.model.move_mode = True
            self.model.active_box_index = clicked_box_idx
            self.model.start_x, self.model.start_y = cx, cy
            self.view.redraw_canvas()
        else:
            # Clicked whitespace
            if not (event.state & 0x0001): # No shift
                self.model.selected_indices.clear()
            
            if self.model.current_class_index != -1:
                self.model.save_state()
                self.model.is_drawing = True
                self.model.start_x, self.model.start_y = cx, cy
            
            self.view.redraw_canvas()

    def apply_image_filter(self):
        if not self.model.output_dir:
            messagebox.showwarning("Warning", "Please set Output Directory first.")
            return
            
        selection = self.view.filter_combo.get()
        if not selection: return
            
        try:
            class_id = int(selection.split(':')[0])
        except ValueError: return

        self.view.root.config(cursor="wait")
        
        def scan_thread():
            def check_file(filename):
                name, _ = os.path.splitext(filename)
                txt_path = os.path.join(self.model.output_dir, name + ".txt")
                if os.path.exists(txt_path):
                    try:
                        with open(txt_path, 'r') as f:
                            for line in f:
                                parts = line.strip().split()
                                if parts and int(parts[0]) == class_id:
                                    return filename
                    except: pass
                return None

            with concurrent.futures.ThreadPoolExecutor() as executor:
                results = list(executor.map(check_file, self.model.full_image_list))
            
            filtered = [r for r in results if r is not None]
            self.view.root.after(0, lambda: self.finish_filter(filtered))

        threading.Thread(target=scan_thread, daemon=True).start()

    def finish_filter(self, filtered_images):
        self.view.root.config(cursor="")
        self.model.image_list = filtered_images
        self.view.file_listbox.delete(0, tk.END)
        for f in self.model.image_list:
            self.view.file_listbox.insert(tk.END, f)
        
        if self.model.image_list:
            self.load_image(0)
        else:
            messagebox.showinfo("Filter", "No images found for this class.")
        self.update_progress_bar()

    def clear_image_filter(self):
        self.model.image_list = list(self.model.full_image_list)
        self.view.file_listbox.delete(0, tk.END)
        for f in self.model.image_list:
            self.view.file_listbox.insert(tk.END, f)
        if self.model.image_list:
            self.load_image(0)
        self.view.filter_combo.set("")
        self.update_progress_bar()

    def on_canvas_drag(self, event):
        if not self.model.current_image_pil: return
        x = self.view.canvas.canvasx(event.x)
        y = self.view.canvas.canvasy(event.y)
        iw, ih = self.model.current_image_pil.size
        
        # Clamp to image boundaries
        min_x = self.view.offset_x
        min_y = self.view.offset_y
        max_x = self.view.offset_x + (iw * self.view.scale)
        max_y = self.view.offset_y + (ih * self.view.scale)
        
        cx = max(min_x, min(max_x, x))
        cy = max(min_y, min(max_y, y))

        if self.model.is_drawing:
            self.view.canvas.delete("temp_rect")
            self.view.canvas.create_rectangle(self.model.start_x, self.model.start_y, cx, cy, 
                                             outline="white", dash=(4, 4), tags="temp_rect")
            
        elif self.model.resize_mode:
            box = self.model.boxes[self.model.active_box_index]
            x1, y1, x2, y2 = denormalize_box(box, iw, ih)
            dx = (cx - self.model.start_x) / self.view.scale
            dy = (cy - self.model.start_y) / self.view.scale
            
            if self.model.active_handle_index == 'nw':
                x1 += dx; y1 += dy
            elif self.model.active_handle_index == 'ne':
                x2 += dx; y1 += dy
            elif self.model.active_handle_index == 'sw':
                x1 += dx; y2 += dy
            elif self.model.active_handle_index == 'se':
                x2 += dx; y2 += dy
            
            # Clamp and normalize
            x1 = max(0, min(iw, x1)); x2 = max(0, min(iw, x2))
            y1 = max(0, min(ih, y1)); y2 = max(0, min(ih, y2))
            
            new_box = normalize_box(x1, y1, x2, y2, iw, ih)
            new_box['class_id'] = box['class_id']
            self.model.boxes[self.model.active_box_index] = new_box
            self.model.start_x, self.model.start_y = cx, cy
            self.view.redraw_canvas()
            
        elif self.model.move_mode:
            box = self.model.boxes[self.model.active_box_index]
            x1, y1, x2, y2 = denormalize_box(box, iw, ih)
            dx = (cx - self.model.start_x) / self.view.scale
            dy = (cy - self.model.start_y) / self.view.scale
            
            # Move all selected boxes
            for idx in self.model.selected_indices:
                b = self.model.boxes[idx]
                x1, y1, x2, y2 = denormalize_box(b, iw, ih)
                w, h = x2 - x1, y2 - y1
                nx1 = x1 + dx; ny1 = y1 + dy
                nx2 = x2 + dx; ny2 = y2 + dy
                
                # Simple clamping for the primary box
                if nx1 < 0: nx2 -= nx1; nx1 = 0
                if ny1 < 0: ny2 -= ny1; ny1 = 0
                if nx2 > iw: nx1 -= (nx2 - iw); nx2 = iw
                if ny2 > ih: ny1 -= (ny2 - ih); ny2 = ih
                
                new_box = normalize_box(nx1, ny1, nx2, ny2, iw, ih)
                new_box['class_id'] = b['class_id']
                self.model.boxes[idx] = new_box
            
            self.model.start_x, self.model.start_y = cx, cy
            self.view.redraw_canvas()

    def on_canvas_release(self, event):
        if not self.model.current_image_pil: return
        x = self.view.canvas.canvasx(event.x)
        y = self.view.canvas.canvasy(event.y)
        iw, ih = self.model.current_image_pil.size
        
        if self.model.is_drawing:
            self.model.is_drawing = False
            self.view.canvas.delete("temp_rect")
            
            # If dragged enough, create box
            if abs(x - self.model.start_x) > 5 or abs(y - self.model.start_y) > 5:
                # Map coords back to image (unclamped x/y for release)
                x1 = (self.model.start_x - self.view.offset_x) / self.view.scale
                y1 = (self.model.start_y - self.view.offset_y) / self.view.scale
                x2 = (x - self.view.offset_x) / self.view.scale
                y2 = (y - self.view.offset_y) / self.view.scale
                
                # Clamp results
                x1 = max(0, min(iw, x1)); x2 = max(0, min(iw, x2))
                y1 = max(0, min(ih, y1)); y2 = max(0, min(ih, y2))
                
                new_box = normalize_box(x1, y1, x2, y2, iw, ih)
                if self.model.current_class_index != -1:
                    new_box['class_id'] = self.model.classes[self.model.current_class_index]['id']
                else:
                    new_box['class_id'] = -1
                
                self.model.boxes.append(new_box)
                self.model.selected_indices = {len(self.model.boxes) - 1}
            else:
                # Stamp template on click if class selected
                if self.model.current_class_index != -1:
                    self.model.save_state()
                    self.stamp_box(x, y)
                    
        self.model.resize_mode = False
        self.model.move_mode = False
        self.view.redraw_canvas()

    def stamp_box(self, x, y):
        if not self.model.current_image_pil: return
        iw, ih = self.model.current_image_pil.size
        cls = self.model.classes[self.model.current_class_index]
        dw = cls.get('default_w', 100)
        dh = cls.get('default_h', 100)
        
        ix = (x - self.view.offset_x) / self.view.scale
        iy = (y - self.view.offset_y) / self.view.scale
        
        x1, y1 = ix - dw/2, iy - dh/2
        x2, y2 = ix + dw/2, iy + dh/2
        
        x1 = max(0, min(iw, x1)); x2 = max(0, min(iw, x2))
        y1 = max(0, min(ih, y1)); y2 = max(0, min(ih, y2))
        
        new_box = normalize_box(x1, y1, x2, y2, iw, ih)
        new_box['class_id'] = cls['id']
        self.model.boxes.append(new_box)
        self.model.selected_indices = {len(self.model.boxes) - 1}
        self.view.redraw_canvas()

    def on_canvas_motion(self, event):
        x = self.view.canvas.canvasx(event.x)
        y = self.view.canvas.canvasy(event.y)
        
        # Update crosshair
        self.view.draw_crosshair(x, y)
        
        # Update status bar coordinates
        if self.model.current_image_pil:
            iw, ih = self.model.current_image_pil.size
            ix = int((x - self.view.offset_x) / self.view.scale)
            iy = int((y - self.view.offset_y) / self.view.scale)
            
            if 0 <= ix <= iw and 0 <= iy <= ih:
                self.view.coords_label.config(text=f"X: {ix}, Y: {iy} | {iw}x{ih}")
            else:
                self.view.coords_label.config(text=f"Out of bounds | {iw}x{ih}")

    def undo(self):
        if self.model.undo():
            self.view.redraw_canvas()
            self.view.update_status("Action undone")

    def redo(self):
        if self.model.redo():
            self.view.redraw_canvas()
            self.view.update_status("Action redone")

    def start_pan(self, event):
        self.view.canvas.scan_mark(event.x, event.y)

    def on_pan(self, event):
        self.view.canvas.scan_dragto(event.x, event.y, gain=1)

    def on_zoom(self, event):
        if not self.model.current_image_pil: return
        
        # 1. Get current mouse position in canvas-space coordinates
        mx = self.view.canvas.canvasx(event.x)
        my = self.view.canvas.canvasy(event.y)
        
        # 2. Convert to image-space coordinates (where on the physical image are we?)
        if self.view.scale == 0: return
        ix = (mx - self.view.offset_x) / self.view.scale
        iy = (my - self.view.offset_y) / self.view.scale
        
        # 3. Update zoom factor
        if event.delta > 0:
            self.model.zoom_factor *= 1.1
        else:
            self.model.zoom_factor /= 1.1
            
        # Limit zoom
        self.model.zoom_factor = max(0.1, min(10.0, self.model.zoom_factor))
            
        # 4. Redraw (fast mode) to calculate new scale and dimensions
        self.view.redraw_canvas(fast=True)
        
        # 5. Re-calculate mouse position in new canvas-space
        new_scale = self.view.scale
        nx = ix * new_scale + self.view.offset_x
        ny = iy * new_scale + self.view.offset_y
        
        # 6. Adjust scroll to keep image point under cursor
        # We want the point (nx, ny) to be at canvas coordinate (event.x, event.y)
        # So the top-left of the visible canvas area should be (nx - event.x, ny - event.y)
        target_vx = nx - event.x
        target_vy = ny - event.y
        
        # Scrollregion dims
        sr_x1, sr_y1, sr_x2, sr_y2 = self.view.canvas.cget('scrollregion').split()
        sr_w = float(sr_x2) - float(sr_x1)
        sr_h = float(sr_y2) - float(sr_y1)
        
        if sr_w > 0: self.view.canvas.xview_moveto(target_vx / sr_w)
        if sr_h > 0: self.view.canvas.yview_moveto(target_vy / sr_h)

        # 7. Debounce high-quality redraw
        if self.redraw_timer:
            self.view.root.after_cancel(self.redraw_timer)
        self.redraw_timer = self.view.root.after(200, lambda: self.view.redraw_canvas(fast=False))

    def find_box_at(self, x, y):
        if not self.model.current_image_pil: return -1
        iw, ih = self.model.current_image_pil.size
        for i, box in enumerate(reversed(self.model.boxes)):
            bx1, by1, bx2, by2 = denormalize_box(box, iw, ih)
            # Scale to canvas
            bx1 = bx1 * self.view.scale + self.view.offset_x
            by1 = by1 * self.view.scale + self.view.offset_y
            bx2 = bx2 * self.view.scale + self.view.offset_x
            by2 = by2 * self.view.scale + self.view.offset_y
            
            # Use small buffer for clicking
            if min(bx1, bx2) - 3 <= x <= max(bx1, bx2) + 3 and min(by1, by2) - 3 <= y <= max(by1, by2) + 3:
                return len(self.model.boxes) - 1 - i
        return -1

    def find_handle_at(self, x, y):
        # We need a halo to make it easier to click small handles
        item = self.view.canvas.find_closest(x, y, halo=5)
        tags = self.view.canvas.gettags(item)
        for tag in tags:
            if tag.startswith("handle_"):
                parts = tag.split("_")
                if len(parts) >= 3:
                    # handle_{index}_{name}
                    return int(parts[1]), parts[2]
        return -1, -1

    def delete_selected_box(self):
        if self._is_input_focused(): return
        if self.model.selected_indices:
            self.model.save_state()
            self.model.boxes = [b for i, b in enumerate(self.model.boxes) if i not in self.model.selected_indices]
            self.model.selected_indices.clear()
            self.view.redraw_canvas()

    def copy_boxes(self):
        if self._is_input_focused(): return
        if self.model.selected_indices:
            self.model.clipboard = [self.model.boxes[i].copy() for i in self.model.selected_indices]
        else:
            self.model.clipboard = [b.copy() for b in self.model.boxes]
        messagebox.showinfo("Info", f"Copied {len(self.model.clipboard)} boxes.")

    def paste_boxes(self):
        if self._is_input_focused(): return
        if not self.model.clipboard: return
        
        self.model.save_state()
        for box in self.model.clipboard:
            self.model.boxes.append(box.copy())
        self.view.redraw_canvas()

    def save_preset(self, slot):
        if self._is_input_focused(): 
            return
            
        print(f"DEBUG: Attempting to save boxes to Preset Slot {slot}...")
            
        if self.model.selected_indices:
            boxes_to_save = [self.model.boxes[i].copy() for i in self.model.selected_indices]
            source = "selected"
        else:
            boxes_to_save = [b.copy() for b in self.model.boxes]
            source = "all"
        
        if not boxes_to_save:
            msg = f"Preset {slot} remains empty (no boxes to save)"
            print(f"DEBUG: {msg}")
            self.view.update_status(msg)
            return

        self.model.presets[slot] = boxes_to_save
        msg = f"Stored {len(boxes_to_save)} {source} boxes in Preset {slot}"
        print(f"DEBUG: {msg}")
        self.view.update_status(msg)

    def apply_preset(self, slot):
        if self._is_input_focused(): 
            return
            
        print(f"DEBUG: Attempting to apply Preset Slot {slot}...")
            
        preset_boxes = self.model.presets.get(slot)
        if not preset_boxes:
            msg = f"Preset {slot} is empty. Use Ctrl+Shift+{slot} to save current boxes."
            print(f"DEBUG: {msg}")
            self.view.update_status(msg)
            return
        
        self.model.save_state()
        for box in preset_boxes:
            self.model.boxes.append(box.copy())
        
        self.view.redraw_canvas()
        msg = f"Applied Preset {slot} ({len(preset_boxes)} boxes)"
        print(f"DEBUG: {msg}")
        self.view.update_status(msg)

    def setup_themes_tab(self, parent):
        """Setup the themes configuration tab."""
        from src.themes import PREDEFINED_THEMES
        
        DarkLabel(parent, text="UI Themes", font=("Segoe UI", 12, "bold")).pack(pady=10)
        
        lb_frame = DarkFrame(parent)
        lb_frame.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)
        
        theme_lb = tk.Listbox(lb_frame, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                               selectbackground=THEME['selection'], relief=tk.FLAT, bd=0)
        theme_lb.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        
        for name in PREDEFINED_THEMES.keys():
            theme_lb.insert(tk.END, f"  {name}")
            
        def on_theme_select(event):
            selection = theme_lb.curselection()
            if selection:
                theme_name = list(PREDEFINED_THEMES.keys())[selection[0]]
                self.model.current_theme_name = theme_name
                self.view.apply_theme(PREDEFINED_THEMES[theme_name])
                # Save to session (optional, handled on close)
                
        theme_lb.bind('<<ListboxSelect>>', on_theme_select)
        
        DarkLabel(parent, text="Select a theme to apply it immediately.", fg=THEME['fg_text']).pack(pady=5)

    def _is_input_focused(self):
        focused = self.view.root.focus_get()
        return isinstance(focused, (tk.Entry, tk.Text))
