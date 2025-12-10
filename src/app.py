import os
import tkinter as tk
from tkinter import filedialog, messagebox, simpledialog, ttk
from PIL import Image, ImageTk
from src.utils import (load_classes, natural_sort_key, parse_yolo, save_yolo, denormalize_box, 
                       normalize_box, load_config, save_config, resize_images_to_lowres,
                       save_classes, create_class_mapping, update_annotation_file, backup_annotations)
from src.ui_components import DarkButton, DarkLabel, DarkListbox, DarkFrame, SectionLabel, SidebarFrame, THEME, DarkEntry
import tkinter.simpledialog as simpledialog
import tkinter.simpledialog as simpledialog
import threading
import concurrent.futures


class AnnotationApp:
    def __init__(self, root):
        self.root = root
        self.root.title("AnnotationTool - Eclipse Theme")
        self.root.geometry("1400x800")
        self.root.configure(bg=THEME['bg_main'])
        
        # State
        self.image_dir = ""
        self.output_dir = ""
        self.image_list = []
        self.full_image_list = [] # Store full list for filtering
        self.current_image_index = -1
        self.current_image = None # PIL Image
        self.tk_image = None # ImageTk
        self.scale = 1.0
        self.offset_x = 0
        self.offset_y = 0
        self.zoom_factor = 1.0

        self.config = load_config("config.json")
        self.classes = load_classes("data/predefined_classes.txt") # List of dicts
        self.filtered_classes = [(i, c) for i, c in enumerate(self.classes)] # List of (original_index, class_dict)
        self.current_class_index = -1 # Idle state by default
        self.template_mode = False # If True, next draw defines template size
        
        # Rendering Cache
        self.cached_dims = None # (width, height)
        self.cached_image_obj = None
        self.is_panning = False
        
        self.boxes = [] # List of dicts (normalized)
        self.selected_indices = set() # Set of ints
        self.clipboard = []
        
        self.is_drawing = False
        self.start_x = 0
        self.start_y = 0
        
        self.resize_mode = False
        self.resize_handle = None # 'nw', 'ne', 'sw', 'se'
        self.resize_box_index = -1
        
        self.move_mode = False
        self.move_box_index = -1
        
        self.auto_save = tk.BooleanVar(value=True)
        self.show_labels = tk.BooleanVar(value=True)
        self.show_right_sidebar = tk.BooleanVar(value=True)
        
        # UI Setup
        self.setup_ui()
        self.bind_events()
        
        # Populate filter combobox
        self.update_filter_combo()

    def update_filter_combo(self):
        values = [f"{c['id']}: {c['name']}" for c in self.classes]
        self.filter_combo['values'] = values
        
    def _is_input_focused(self):
        """Check if an input widget has focus"""
        try:
            focus = self.root.focus_get()
            return isinstance(focus, (tk.Entry, tk.Text, ttk.Entry))
        except:
            return False

    def apply_image_filter(self):
        if not self.output_dir:
            messagebox.showwarning("Warning", "Please set Output Directory first to filter by annotations.")
            return
            
        selection = self.filter_combo.get()
        if not selection:
            messagebox.showwarning("Warning", "Please select a class to filter by.")
            return
            
        try:
            class_id = int(selection.split(':')[0])
        except ValueError:
            return

        # Disable UI during scan
        self.root.config(cursor="wait")
        
        def scan_thread():
            filtered_images = []
            total = len(self.full_image_list)
            
            # Use threading for faster IO
            def check_file(filename):
                name, _ = os.path.splitext(filename)
                txt_path = os.path.join(self.output_dir, name + ".txt")
                if os.path.exists(txt_path):
                    try:
                        with open(txt_path, 'r') as f:
                            for line in f:
                                parts = line.strip().split()
                                if parts and int(parts[0]) == class_id:
                                    return filename
                    except:
                        pass
                return None

            with concurrent.futures.ThreadPoolExecutor() as executor:
                results = list(executor.map(check_file, self.full_image_list))
            
            filtered_images = [r for r in results if r is not None]
            
            # Update UI on main thread
            self.root.after(0, lambda: self.finish_filter(filtered_images, selection))

        threading.Thread(target=scan_thread, daemon=True).start()

    def finish_filter(self, filtered_images, class_name):
        self.root.config(cursor="")
        self.image_list = filtered_images
        self.image_list.sort(key=natural_sort_key)
        
        self.file_listbox.delete(0, tk.END)
        for f in self.image_list:
            self.file_listbox.insert(tk.END, f)
            
        if self.image_list:
            self.load_image(0)
        else:
            self.current_image = None
            self.canvas.delete("all")
            self.root.title("AnnotationTool - No images found with class " + class_name)
            
        messagebox.showinfo("Filter Result", f"Found {len(filtered_images)} images containing {class_name}")

    def clear_image_filter(self):
        self.image_list = list(self.full_image_list)
        self.image_list.sort(key=natural_sort_key)
        
        self.file_listbox.delete(0, tk.END)
        for f in self.image_list:
            self.file_listbox.insert(tk.END, f)
            
        if self.image_list:
            self.load_image(0)
        self.filter_combo.set("")
        
    def setup_ui(self):
        # Toolbar (Top) for toggles
        self.toolbar = DarkFrame(self.root, height=30)
        self.toolbar.pack(fill=tk.X, side=tk.TOP)
        
        DarkButton(self.toolbar, text="Toggle Box List", command=self.toggle_right_sidebar).pack(side=tk.RIGHT, padx=5, pady=2)
        
        # Main Layout: Sidebar (Left) + Canvas Area (Center) + Sidebar (Right)
        self.main_container = tk.PanedWindow(self.root, orient=tk.HORIZONTAL, bg=THEME['bg_main'], sashwidth=4, sashrelief=tk.FLAT)
        self.main_container.pack(fill=tk.BOTH, expand=True)
        
        # Left Sidebar
        self.sidebar = SidebarFrame(self.main_container, width=250)
        self.main_container.add(self.sidebar, minsize=200)
        
        # Canvas Area
        self.canvas_frame = DarkFrame(self.main_container)
        self.main_container.add(self.canvas_frame, minsize=400, stretch="always")
        
        # Right Sidebar
        self.right_sidebar = SidebarFrame(self.main_container, width=250)
        self.main_container.add(self.right_sidebar, minsize=200)
        
        self.setup_sidebar()
        self.setup_canvas()
        self.setup_right_sidebar()
        
    def setup_sidebar(self):
        # Project Section
        SectionLabel(self.sidebar, text="Project").pack(fill=tk.X, padx=10, pady=(10, 0))
        
        btn_frame = DarkFrame(self.sidebar, bg=THEME['bg_sidebar'])
        btn_frame.pack(fill=tk.X, padx=10, pady=5)
        
        DarkButton(btn_frame, text="Open Images", command=self.select_image_dir).pack(fill=tk.X, pady=2)
        DarkButton(btn_frame, text="Set Output Dir", command=self.select_output_dir).pack(fill=tk.X, pady=2)
        
        self.dir_label = DarkLabel(self.sidebar, text="No directory selected", bg=THEME['bg_sidebar'], fg=THEME['fg_text'], wraplength=230)
        self.dir_label.pack(fill=tk.X, padx=10, pady=5)
        
        # Classes Section
        SectionLabel(self.sidebar, text="Classes").pack(fill=tk.X, padx=10, pady=(10, 0))
        
        class_btn_frame = DarkFrame(self.sidebar, bg=THEME['bg_sidebar'])
        class_btn_frame.pack(fill=tk.X, padx=10, pady=5)
        
        DarkButton(class_btn_frame, text="Create Template", command=self.enter_template_mode).pack(fill=tk.X, pady=2)
        
        # Search Bar
        self.class_search_var = tk.StringVar()
        self.class_search_var.trace("w", self.filter_classes)
        search_frame = DarkFrame(self.sidebar)
        search_frame.pack(fill=tk.X, padx=10, pady=(5, 0))
        DarkEntry(search_frame, textvariable=self.class_search_var).pack(fill=tk.X)
        
        self.class_listbox = DarkListbox(self.sidebar, height=10)
        self.class_listbox.pack(fill=tk.X, padx=10, pady=5)
        self.class_listbox.bind('<<ListboxSelect>>', self.on_class_select)
        
        # Configure selection colors for "flipped" look (Light BG, Dark FG)
        self.class_listbox.configure(selectbackground='#cccccc', selectforeground='#252526')
        
        self.update_class_list()
        
        # Tools Section
        SectionLabel(self.sidebar, text="Tools").pack(fill=tk.X, padx=10, pady=(10, 0))
        
        DarkButton(self.sidebar, text="Copy Boxes (Ctrl+C)", command=self.copy_boxes).pack(fill=tk.X, padx=10, pady=2)
        DarkButton(self.sidebar, text="Paste Boxes (Ctrl+V)", command=self.paste_boxes).pack(fill=tk.X, padx=10, pady=2)
        
        tk.Checkbutton(self.sidebar, text="Auto Save", variable=self.auto_save, 
                       bg=THEME['bg_sidebar'], fg=THEME['fg_text'], selectcolor=THEME['bg_sidebar'], activebackground=THEME['bg_sidebar'], activeforeground=THEME['fg_highlight']).pack(anchor='w', padx=10, pady=5)
        
        tk.Checkbutton(self.sidebar, text="Show Labels", variable=self.show_labels, command=self.redraw_canvas,
                       bg=THEME['bg_sidebar'], fg=THEME['fg_text'], selectcolor=THEME['bg_sidebar'], activebackground=THEME['bg_sidebar'], activeforeground=THEME['fg_highlight']).pack(anchor='w', padx=10, pady=5)

        DarkButton(self.sidebar, text="Settings", command=self.open_settings_dialog).pack(fill=tk.X, padx=10, pady=(10, 2))

        # File List
        SectionLabel(self.sidebar, text="Files").pack(fill=tk.X, padx=10, pady=(10, 0))
        
        # Filter Section
        filter_frame = DarkFrame(self.sidebar, bg=THEME['bg_sidebar'])
        filter_frame.pack(fill=tk.X, padx=10, pady=5)
        
        self.filter_var = tk.StringVar()
        self.filter_combo = ttk.Combobox(filter_frame, textvariable=self.filter_var, state="readonly")
        self.filter_combo.pack(fill=tk.X, pady=2)
        
        btn_filter_frame = DarkFrame(filter_frame, bg=THEME['bg_sidebar'])
        btn_filter_frame.pack(fill=tk.X, pady=2)
        
        DarkButton(btn_filter_frame, text="Filter", command=self.apply_image_filter, width=8).pack(side=tk.LEFT, padx=(0, 2), expand=True, fill=tk.X)
        DarkButton(btn_filter_frame, text="Clear", command=self.clear_image_filter, width=8).pack(side=tk.RIGHT, padx=(2, 0), expand=True, fill=tk.X)

        # Container for listbox and scrollbar
        file_list_container = DarkFrame(self.sidebar, bg=THEME['bg_sidebar'])
        file_list_container.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)
        
        self.file_listbox = DarkListbox(file_list_container, height=15)
        self.file_listbox.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        
        file_scrollbar = tk.Scrollbar(file_list_container, orient=tk.VERTICAL, command=self.file_listbox.yview)
        file_scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        
        self.file_listbox.configure(yscrollcommand=file_scrollbar.set)
        self.file_listbox.bind('<<ListboxSelect>>', self.on_file_select)

    def setup_right_sidebar(self):
        SectionLabel(self.right_sidebar, text="Box Labels").pack(fill=tk.X, padx=10, pady=(10, 0))
        
        self.box_listbox = DarkListbox(self.right_sidebar, selectmode=tk.EXTENDED)
        self.box_listbox.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)
        self.box_listbox.bind('<<ListboxSelect>>', self.on_box_list_select)

    def toggle_right_sidebar(self):
        if self.show_right_sidebar.get():
            self.main_container.remove(self.right_sidebar)
            self.show_right_sidebar.set(False)
        else:
            self.main_container.add(self.right_sidebar, minsize=200)
            self.show_right_sidebar.set(True)

    def setup_canvas(self):
        # Canvas Frame needs grid layout for scrollbars
        self.canvas_frame.grid_rowconfigure(0, weight=1)
        self.canvas_frame.grid_columnconfigure(0, weight=1)
        
        self.canvas = tk.Canvas(self.canvas_frame, bg="#111111", highlightthickness=0)
        self.canvas.grid(row=0, column=0, sticky="nsew")
        
        # Scrollbars
        self.v_scroll = tk.Scrollbar(self.canvas_frame, orient=tk.VERTICAL, command=self.canvas.yview)
        self.v_scroll.grid(row=0, column=1, sticky="ns")
        
        self.h_scroll = tk.Scrollbar(self.canvas_frame, orient=tk.HORIZONTAL, command=self.canvas.xview)
        self.h_scroll.grid(row=1, column=0, sticky="ew")
        
        self.canvas.configure(yscrollcommand=self.v_scroll.set, xscrollcommand=self.h_scroll.set)
        
        self.canvas.bind("<Button-1>", self.on_canvas_click)
        self.canvas.bind("<B1-Motion>", self.on_canvas_drag)
        self.canvas.bind("<ButtonRelease-1>", self.on_canvas_release)
        self.canvas.bind("<Configure>", self.on_canvas_resize)
        
        # Zoom & Pan
        self.canvas.bind("<Control-MouseWheel>", self.on_zoom)
        self.canvas.bind("<Button-2>", self.start_pan)
        self.canvas.bind("<Control-MouseWheel>", self.on_zoom)
        self.canvas.bind("<Button-2>", self.start_pan)
        self.canvas.bind("<B2-Motion>", self.pan_image)
        self.canvas.bind("<ButtonRelease-2>", self.stop_pan)
        
        # Mouse motion for grid lines
        self.canvas.bind("<Motion>", self.on_canvas_motion)
        
        # Keyboard focus
        self.canvas.bind("<Button-1>", lambda e: self.canvas.focus_set(), add="+")

    def bind_events(self):
        self.root.bind(self.config['prev_image'], lambda e: self.prev_image())
        self.root.bind(self.config['next_image'], lambda e: self.next_image())
        self.root.bind(self.config['cycle_class'], lambda e: self.cycle_class())
        self.root.bind(self.config['delete_box'], lambda e: self.delete_selected_box())
        self.root.bind(self.config['copy'], lambda e: self.copy_boxes())
        self.root.bind(self.config['paste'], lambda e: self.paste_boxes())
        self.root.bind(self.config['edit_class'], lambda e: self.edit_selected_box_class())
        self.root.bind(self.config['deselect'], lambda e: self.deselect_class())
        
        # Keep arrow keys as hardcoded navigation alternatives or add to config?
        # Let's keep them as hardcoded secondary options for now, or just rely on config.
        # User asked for configurable shortcuts, so let's stick to config primarily.
        # But Left/Right are standard. Let's leave them.
        self.root.bind("<Left>", lambda e: self.prev_image())
        self.root.bind("<Right>", lambda e: self.next_image())

    def deselect_class(self):
        if self._is_input_focused(): return
        self.current_class_index = -1
        self.class_listbox.selection_clear(0, tk.END)
        self.template_mode = False
        self.update_cursor()
        self.redraw_canvas()
        messagebox.showinfo("Info", "Idle Mode: Class deselected.")

    def open_settings_dialog(self):
        top = tk.Toplevel(self.root)
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
        
        # Setup Keybindings Tab
        self.setup_keybindings_tab(keybindings_tab, top)
        
        # Setup Class Management Tab
        self.setup_class_management_tab(class_mgmt_tab)
        
        # Setup Batch Operations Tab
        self.setup_batch_operations_tab(batch_ops_tab)
    
    def setup_keybindings_tab(self, parent, window):
        """Setup the keybindings configuration tab"""
        DarkLabel(parent, text="Configure Keybindings", font=("Segoe UI", 12, "bold")).pack(pady=10)
        
        # Scrollable Frame for keys
        canvas = tk.Canvas(parent, bg=THEME['bg_main'], highlightthickness=0)
        scrollbar = tk.Scrollbar(parent, orient="vertical", command=canvas.yview)
        scrollable_frame = DarkFrame(canvas)
        
        scrollable_frame.bind(
            "<Configure>",
            lambda e: canvas.configure(scrollregion=canvas.bbox("all"))
        )
        
        canvas.create_window((0, 0), window=scrollable_frame, anchor="nw")
        canvas.configure(yscrollcommand=scrollbar.set)
        
        canvas.pack(side="left", fill="both", expand=True, padx=10)
        scrollbar.pack(side="right", fill="y")
        
        # Key Entries
        self.key_entries = {}
        row = 0
        for action, key in self.config.items():
            DarkLabel(scrollable_frame, text=action.replace("_", " ").title()).grid(row=row, column=0, sticky="w", pady=5, padx=5)
            
            btn = DarkButton(scrollable_frame, text=key, width=15)
            btn.grid(row=row, column=1, sticky="e", pady=5, padx=5)
            btn.configure(command=lambda b=btn, a=action: self.capture_key(b, a))
            
            row += 1
            
        # Save Button
        DarkButton(parent, text="Save & Close", command=lambda: self.save_settings(window)).pack(pady=10)
    
    def setup_class_management_tab(self, parent):
        """Setup the class management tab"""
        DarkLabel(parent, text="Manage Classes", font=("Segoe UI", 12, "bold")).pack(pady=10)
        
        # Instructions
        info_text = "Add, remove, or reorder classes. Changes will update all annotation files."
        DarkLabel(parent, text=info_text, wraplength=550, fg=THEME['fg_text']).pack(pady=5)
        
        # Main container
        main_frame = DarkFrame(parent)
        main_frame.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        
        # Left side: Class list
        list_frame = DarkFrame(main_frame)
        list_frame.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=(0, 10))
        
        DarkLabel(list_frame, text="Current Classes:", font=("Segoe UI", 10, "bold")).pack(anchor="w", pady=5)
        
        # Listbox with scrollbar
        list_container = DarkFrame(list_frame)
        list_container.pack(fill=tk.BOTH, expand=True)
        
        self.class_mgmt_listbox = DarkListbox(list_container, height=15)
        self.class_mgmt_listbox.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        
        scrollbar = tk.Scrollbar(list_container, orient=tk.VERTICAL, command=self.class_mgmt_listbox.yview)
        scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        self.class_mgmt_listbox.configure(yscrollcommand=scrollbar.set)
        
        # Populate with current classes
        self.temp_classes = [c['name'] for c in self.classes]
        self.update_class_mgmt_list()
        
        # Right side: Buttons
        button_frame = DarkFrame(main_frame)
        button_frame.pack(side=tk.RIGHT, fill=tk.Y)
        
        DarkLabel(button_frame, text="Actions:", font=("Segoe UI", 10, "bold")).pack(anchor="w", pady=5)
        
        # Add class
        add_frame = DarkFrame(button_frame)
        add_frame.pack(fill=tk.X, pady=5)
        
        DarkLabel(add_frame, text="New Class:").pack(anchor="w")
        self.new_class_entry = tk.Entry(add_frame, bg=THEME['entry_bg'], fg=THEME['entry_fg'], 
                                        insertbackground=THEME['fg_text'], relief=tk.FLAT, bd=5)
        self.new_class_entry.pack(fill=tk.X, pady=2)
        
        DarkButton(add_frame, text="Add Class", command=self.add_class_to_temp_list).pack(fill=tk.X, pady=2)
        
        # Remove, Move Up, Move Down
        DarkButton(button_frame, text="Remove Selected", command=self.remove_class_from_temp_list).pack(fill=tk.X, pady=5)
        DarkButton(button_frame, text="Move Up", command=self.move_class_up).pack(fill=tk.X, pady=2)
        DarkButton(button_frame, text="Move Down", command=self.move_class_down).pack(fill=tk.X, pady=2)
        
        # Separator
        tk.Frame(button_frame, height=2, bg=THEME['border']).pack(fill=tk.X, pady=10)
        
        # Apply changes button
        DarkButton(button_frame, text="Apply Changes", command=self.apply_class_changes, 
                  bg="#007acc", fg="#ffffff").pack(fill=tk.X, pady=5)
    
    def update_class_mgmt_list(self):
        """Update the class management listbox"""
        self.class_mgmt_listbox.delete(0, tk.END)
        for i, class_name in enumerate(self.temp_classes):
            self.class_mgmt_listbox.insert(tk.END, f"{i}: {class_name}")
    
    def add_class_to_temp_list(self):
        """Add a new class to the temporary list"""
        new_name = self.new_class_entry.get().strip()
        if not new_name:
            messagebox.showwarning("Warning", "Please enter a class name.")
            return
        
        if new_name in self.temp_classes:
            messagebox.showwarning("Warning", "Class already exists.")
            return
        
        self.temp_classes.append(new_name)
        self.update_class_mgmt_list()
        self.new_class_entry.delete(0, tk.END)
    
    def remove_class_from_temp_list(self):
        """Remove selected class from the temporary list"""
        selection = self.class_mgmt_listbox.curselection()
        if not selection:
            messagebox.showwarning("Warning", "Please select a class to remove.")
            return
        
        index = selection[0]
        class_name = self.temp_classes[index]
        
        confirm = messagebox.askyesno("Confirm Removal", 
                                      f"Remove class '{class_name}'?\n\n"
                                      "This will update all annotation files and remove annotations with this class.")
        if confirm:
            self.temp_classes.pop(index)
            self.update_class_mgmt_list()
    
    def move_class_up(self):
        """Move selected class up in the list"""
        selection = self.class_mgmt_listbox.curselection()
        if not selection:
            messagebox.showwarning("Warning", "Please select a class to move.")
            return
        
        index = selection[0]
        if index == 0:
            messagebox.showinfo("Info", "Class is already at the top.")
            return
        
        # Swap
        self.temp_classes[index], self.temp_classes[index - 1] = self.temp_classes[index - 1], self.temp_classes[index]
        self.update_class_mgmt_list()
        self.class_mgmt_listbox.selection_set(index - 1)
    
    def move_class_down(self):
        """Move selected class down in the list"""
        selection = self.class_mgmt_listbox.curselection()
        if not selection:
            messagebox.showwarning("Warning", "Please select a class to move.")
            return
        
        index = selection[0]
        if index == len(self.temp_classes) - 1:
            messagebox.showinfo("Info", "Class is already at the bottom.")
            return
        
        # Swap
        self.temp_classes[index], self.temp_classes[index + 1] = self.temp_classes[index + 1], self.temp_classes[index]
        self.update_class_mgmt_list()
        self.class_mgmt_listbox.selection_set(index + 1)
    
    def apply_class_changes(self):
        """Apply class changes and update all annotation files"""
        if not self.temp_classes:
            messagebox.showerror("Error", "Cannot save empty class list.")
            return
        
        # Get old class names
        old_classes = [c['name'] for c in self.classes]
        
        # Check if there are any changes
        if old_classes == self.temp_classes:
            messagebox.showinfo("Info", "No changes to apply.")
            return
        
        # Count annotation files
        txt_count = 0
        if self.image_dir:
            for f in os.listdir(self.image_dir):
                if f.lower().endswith('.txt'):
                    txt_count += 1
        
        # Confirm with user
        confirm_msg = f"Apply class changes?\n\n"
        confirm_msg += f"Old classes: {len(old_classes)}\n"
        confirm_msg += f"New classes: {len(self.temp_classes)}\n"
        confirm_msg += f"Annotation files to update: {txt_count}\n\n"
        confirm_msg += "A backup will be created before making changes."
        
        if not messagebox.askyesno("Confirm Changes", confirm_msg):
            return
        
        # Create backup if we have an image directory
        backup_path = None
        if self.image_dir and txt_count > 0:
            backup_path = backup_annotations(self.image_dir)
            if backup_path:
                messagebox.showinfo("Backup Created", f"Backup created at:\n{os.path.basename(backup_path)}")
            else:
                if not messagebox.askyesno("Warning", "Failed to create backup. Continue anyway?"):
                    return
        
        # Create class mapping
        class_mapping = create_class_mapping(old_classes, self.temp_classes)
        
        # Update all annotation files
        if self.image_dir and txt_count > 0:
            updated_count = 0
            failed_count = 0
            
            for filename in os.listdir(self.image_dir):
                if filename.lower().endswith('.txt'):
                    txt_path = os.path.join(self.image_dir, filename)
                    if update_annotation_file(txt_path, class_mapping):
                        updated_count += 1
                    else:
                        failed_count += 1
            
            result_msg = f"Updated {updated_count} annotation files."
            if failed_count > 0:
                result_msg += f"\nFailed to update {failed_count} files."
            messagebox.showinfo("Update Complete", result_msg)
        
        # Save new class list
        save_classes("data/predefined_classes.txt", self.temp_classes)
        
        # Reload classes in the application
        self.classes = load_classes("data/predefined_classes.txt")
        self.class_search_var.set("") # Clear search
        self.filtered_classes = [(i, c) for i, c in enumerate(self.classes)]
        self.update_class_list()
        
        # Reload current image to reflect changes
        if self.current_image_index != -1:
            self.load_image(self.current_image_index)
        
        messagebox.showinfo("Success", "Class changes applied successfully!")

    def setup_batch_operations_tab(self, parent):
        """Setup the batch operations tab for replacing class IDs"""
        DarkLabel(parent, text="Batch Replace Class IDs", font=("Segoe UI", 12, "bold")).pack(pady=10)
        
        # Instructions
        info_text = "Replace all instances of one class ID with another in the current directory.\nUseful for fixing mislabeled annotations."
        DarkLabel(parent, text=info_text, wraplength=550, fg=THEME['fg_text']).pack(pady=5)
        
        # Warning
        warning_frame = DarkFrame(parent, bg="#3d2a00")
        warning_frame.pack(fill=tk.X, padx=10, pady=10)
        DarkLabel(warning_frame, text="⚠ Warning: This will modify all annotation files in the current directory!", 
                 fg="#ffcc00", font=("Segoe UI", 9, "bold"), bg="#3d2a00").pack(pady=5)
        
        # Main container
        main_frame = DarkFrame(parent)
        main_frame.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        
        # Current directory display
        dir_frame = DarkFrame(main_frame)
        dir_frame.pack(fill=tk.X, pady=(0, 15))
        
        DarkLabel(dir_frame, text="Current Directory:", font=("Segoe UI", 9, "bold")).pack(anchor="w")
        self.batch_dir_label = DarkLabel(dir_frame, text=self.image_dir if self.image_dir else "No directory loaded", 
                                        fg=THEME['fg_highlight'], wraplength=550)
        self.batch_dir_label.pack(anchor="w", padx=10)
        
        # Initialize selected class variables
        self.batch_selected_old_id = None
        self.batch_selected_new_id = None
        
        # Selection frame
        selection_frame = DarkFrame(main_frame)
        selection_frame.pack(fill=tk.X, pady=10)
        
        # Old Class ID
        old_class_frame = DarkFrame(selection_frame)
        old_class_frame.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=(0, 10))
        
        DarkLabel(old_class_frame, text="Old Class ID (to replace):", font=("Segoe UI", 9, "bold")).pack(anchor="w", pady=(0, 5))
        
        # Listbox for old class
        old_list_container = DarkFrame(old_class_frame)
        old_list_container.pack(fill=tk.BOTH, expand=True)
        
        self.batch_old_listbox = DarkListbox(old_list_container, height=10)
        self.batch_old_listbox.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        
        old_scrollbar = tk.Scrollbar(old_list_container, orient=tk.VERTICAL, command=self.batch_old_listbox.yview)
        old_scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        self.batch_old_listbox.configure(yscrollcommand=old_scrollbar.set)
        
        # Submit button for old class
        DarkButton(old_class_frame, text="Select Old Class", command=self.submit_old_class,
                  bg="#555555", fg="#ffffff").pack(fill=tk.X, pady=(5, 0))
        
        # Selected old class display
        self.batch_old_selected_label = DarkLabel(old_class_frame, text="Selected: None", 
                                                  fg="#00ff00", font=("Segoe UI", 9, "bold"))
        self.batch_old_selected_label.pack(anchor="w", pady=(5, 0))
        
        # New Class ID
        new_class_frame = DarkFrame(selection_frame)
        new_class_frame.pack(side=tk.RIGHT, fill=tk.BOTH, expand=True)
        
        DarkLabel(new_class_frame, text="New Class ID (replacement):", font=("Segoe UI", 9, "bold")).pack(anchor="w", pady=(0, 5))
        
        # Listbox for new class
        new_list_container = DarkFrame(new_class_frame)
        new_list_container.pack(fill=tk.BOTH, expand=True)
        
        self.batch_new_listbox = DarkListbox(new_list_container, height=10)
        self.batch_new_listbox.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        
        new_scrollbar = tk.Scrollbar(new_list_container, orient=tk.VERTICAL, command=self.batch_new_listbox.yview)
        new_scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        self.batch_new_listbox.configure(yscrollcommand=new_scrollbar.set)
        
        # Submit button for new class
        DarkButton(new_class_frame, text="Select New Class", command=self.submit_new_class,
                  bg="#555555", fg="#ffffff").pack(fill=tk.X, pady=(5, 0))
        
        # Selected new class display
        self.batch_new_selected_label = DarkLabel(new_class_frame, text="Selected: None", 
                                                  fg="#00ff00", font=("Segoe UI", 9, "bold"))
        self.batch_new_selected_label.pack(anchor="w", pady=(5, 0))
        
        # Populate both listboxes with current classes
        for i, cls in enumerate(self.classes):
            display_text = f"{cls['id']}: {cls['name']}"
            self.batch_old_listbox.insert(tk.END, display_text)
            self.batch_new_listbox.insert(tk.END, display_text)
        
        # Button frame
        button_frame = DarkFrame(main_frame)
        button_frame.pack(fill=tk.X, pady=(15, 0))
        
        # Execute button
        DarkButton(button_frame, text="Execute Batch Replace", command=self.execute_batch_replace,
                  bg="#007acc", fg="#ffffff", font=("Segoe UI", 10, "bold")).pack(fill=tk.X)
    
    def submit_old_class(self):
        """Submit the selected old class"""
        selection = self.batch_old_listbox.curselection()
        if not selection:
            messagebox.showwarning("Warning", "Please select a class from the list first.")
            return
        
        self.batch_selected_old_id = selection[0]
        class_info = self.classes[self.batch_selected_old_id]
        self.batch_old_selected_label.config(text=f"Selected: {class_info['id']} - {class_info['name']}")
    
    def submit_new_class(self):
        """Submit the selected new class"""
        selection = self.batch_new_listbox.curselection()
        if not selection:
            messagebox.showwarning("Warning", "Please select a class from the list first.")
            return
        
        self.batch_selected_new_id = selection[0]
        class_info = self.classes[self.batch_selected_new_id]
        self.batch_new_selected_label.config(text=f"Selected: {class_info['id']} - {class_info['name']}")
    
    def execute_batch_replace(self):
        """Execute the batch replace operation"""
        # Check if directory is loaded
        if not self.image_dir:
            messagebox.showerror("Error", "No directory loaded. Please open a directory first.")
            return
        
        # Check if selections have been submitted
        if self.batch_selected_old_id is None:
            messagebox.showwarning("Warning", "Please select and submit the old class ID to replace.")
            return
        
        if self.batch_selected_new_id is None:
            messagebox.showwarning("Warning", "Please select and submit the new class ID.")
            return
        
        old_class_id = self.classes[self.batch_selected_old_id]['id']
        new_class_id = self.classes[self.batch_selected_new_id]['id']
        
        old_class_name = self.classes[self.batch_selected_old_id]['name']
        new_class_name = self.classes[self.batch_selected_new_id]['name']
        
        if old_class_id == new_class_id:
            messagebox.showwarning("Warning", "Old and new class IDs are the same. No changes needed.")
            return
        
        # Count files that will be affected
        txt_files = [f for f in os.listdir(self.image_dir) if f.lower().endswith('.txt') and f != 'classes.txt']
        
        if not txt_files:
            messagebox.showinfo("Info", "No annotation files found in the directory.")
            return
        
        # Confirm with user
        confirm_msg = f"Batch Replace Class IDs\n\n"
        confirm_msg += f"Old Class: {old_class_id} - {old_class_name}\n"
        confirm_msg += f"New Class: {new_class_id} - {new_class_name}\n\n"
        confirm_msg += f"Directory: {os.path.basename(self.image_dir)}\n"
        confirm_msg += f"Files to process: {len(txt_files)}\n\n"
        confirm_msg += "A backup will be created before making changes.\n\n"
        confirm_msg += "Do you want to continue?"
        
        if not messagebox.askyesno("Confirm Batch Replace", confirm_msg):
            return
        
        # Create backup
        backup_path = backup_annotations(self.image_dir)
        if backup_path:
            messagebox.showinfo("Backup Created", f"Backup created at:\n{os.path.basename(backup_path)}")
        else:
            if not messagebox.askyesno("Warning", "Failed to create backup. Continue anyway?"):
                return
        
        # Execute batch replace
        count = 0
        files_modified = 0
        
        for filename in txt_files:
            file_path = os.path.join(self.image_dir, filename)
            
            try:
                with open(file_path, 'r') as f:
                    lines = f.readlines()
                
                new_lines = []
                modified = False
                
                for line in lines:
                    parts = line.strip().split()
                    if len(parts) >= 5:
                        try:
                            current_id = int(parts[0])
                            if current_id == old_class_id:
                                # Replace the class ID
                                parts[0] = str(new_class_id)
                                new_line = " ".join(parts) + "\n"
                                new_lines.append(new_line)
                                modified = True
                                count += 1
                            else:
                                new_lines.append(line)
                        except ValueError:
                            new_lines.append(line)
                    else:
                        new_lines.append(line)
                
                if modified:
                    with open(file_path, 'w') as f:
                        f.writelines(new_lines)
                    files_modified += 1
                    
            except Exception as e:
                messagebox.showerror("Error", f"Error processing {filename}: {e}")
                return
        
        # Show results
        result_msg = f"Batch Replace Complete!\n\n"
        result_msg += f"Files modified: {files_modified}\n"
        result_msg += f"Labels replaced: {count}\n\n"
        result_msg += f"Changed from: {old_class_name} (ID {old_class_id})\n"
        result_msg += f"Changed to: {new_class_name} (ID {new_class_id})"
        
        messagebox.showinfo("Success", result_msg)
        
        # Reload current image to reflect changes
        if self.current_image_index != -1:
            self.load_image(self.current_image_index)


    def capture_key(self, button, action):
        button.config(text="Press any key...")
        
        def on_key(event):
            keysym = event.keysym
            
            # Ignore modifier keys themselves (e.g. if user just presses Ctrl)
            if keysym in ('Control_L', 'Control_R', 'Shift_L', 'Shift_R', 'Alt_L', 'Alt_R', 'Caps_Lock'):
                return

            # Modifiers
            parts = []
            # Control: 0x4
            if event.state & 0x0004: 
                parts.append("Control")
            
            # Alt: Check 0x20000 (131072) for Windows or 0x8 for Linux/Mac?
            # On Windows, 0x8 is NOT Alt. 0x20000 is. 
            # Safest is to check 0x20000. 
            # If we are cross-platform we might check both, but 0x8 overlaps with other things on Windows sometimes.
            # Given user is on Windows, we check 0x20000.
            if event.state & 131072: 
                parts.append("Alt")
            
            # Shift: 0x1
            if event.state & 0x0001: 
                parts.append("Shift")
            
            # Construct key
            if len(keysym) > 1:
                key_part = f"<{keysym}>" # e.g. <Right>, <Delete>
            else:
                key_part = keysym.lower() # e.g. a, b, c
            
            if parts:
                # If modifiers exist, e.g. <Control-c>, <Alt-Right>
                # Format: <Modifier-Modifier-Key>
                # Key part inside shouldn't have <> if char? Actually Tkinter format is <Control-c>
                # But for special keys <Control-Right>
                
                # If key_part already has <>, remove them for valid syntax? 
                # e.g. <Control-<Right>> is INVALID. Should be <Control-Right>.
                if key_part.startswith("<") and key_part.endswith(">"):
                    key_part = key_part[1:-1]
                
                new_key = f"<{''.join([p + '-' for p in parts])}{key_part}>"
            else:
                # No modifiers
                # If it's a char, wrap it: <a>
                # If it's special: <Right>
                if len(keysym) == 1:
                    new_key = f"<{keysym}>"
                else:
                    new_key = f"<{keysym}>"

            self.config[action] = new_key
            button.config(text=new_key)
            self.settings_capture_window.destroy()

        # Create a transparent overlay or just bind to top window?
        # Let's bind to the button itself or the top window
        # But we need to grab focus
        
        # Easier: Simple dialog asking to press key
        # But we want inline.
        
        # Let's use a capture dialog
        cap = tk.Toplevel(self.root)
        cap.title("Press Key")
        cap.geometry("200x100")
        DarkLabel(cap, text=f"Press key for '{action}'...").pack(expand=True)
        
        self.settings_capture_window = cap
        
        cap.bind("<Key>", on_key)
        cap.focus_set()

    def save_settings(self, window):
        save_config("config.json", self.config)
        self.bind_events() # Rebind
        window.destroy()
        messagebox.showinfo("Settings", "Keybindings saved!")

    # --- Project Management ---
    def select_image_dir(self):
        path = filedialog.askdirectory(title="Select Image Directory")
        if path:
            # Save current work before switching directories
            if self.current_image_index != -1 and self.auto_save.get() and self.image_dir:
                self.save_annotations()
            
            # Reset state when loading new directory
            self.current_image_index = -1
            self.boxes = []
            self.selected_indices = set()
            self.current_image = None
            self.tk_image = None
            
            # Ask user if they want to lower resolution
            response = messagebox.askyesno(
                "Lower Resolution?",
                "Do you want to lower the resolution of images before loading?\n\n"
                "This will create a new directory with '_lowres' suffix containing "
                "resized images (720px -> 1920x1080).\n\n"
                "Original images will remain untouched."
            )
            
            if response:
                # Show processing message
                messagebox.showinfo("Processing", "Processing images... This may take a moment.")
                
                # Resize images
                lowres_path = resize_images_to_lowres(path)
                
                if lowres_path:
                    self.image_dir = lowres_path
                    messagebox.showinfo("Success", f"Images processed successfully!\n\nLoading from: {os.path.basename(lowres_path)}")
                else:
                    messagebox.showerror("Error", "Failed to process images. Loading original directory instead.")
                    self.image_dir = path
            else:
                self.image_dir = path
            
            self.load_images()
            self.update_dir_label()

    def select_output_dir(self):
        path = filedialog.askdirectory(title="Select Output Directory")
        if path:
            self.output_dir = path
            self.update_dir_label()

    def update_dir_label(self):
        text = f"Img: {os.path.basename(self.image_dir)}\nOut: {os.path.basename(self.output_dir)}"
        self.dir_label.config(text=text)

    def load_images(self):
        self.image_list = []
        extensions = ('.jpg', '.jpeg', '.png', '.bmp')
        for f in os.listdir(self.image_dir):
            if f.lower().endswith(extensions):
                self.image_list.append(f)
        
        self.image_list.sort(key=natural_sort_key)
        self.full_image_list = list(self.image_list) # Keep a copy of full list
        
        self.file_listbox.delete(0, tk.END)
        for f in self.image_list:
            self.file_listbox.insert(tk.END, f)
            
        if self.image_list:
            self.load_image(0)
        else:
            messagebox.showinfo("Info", "No images found in directory.")

    # --- Image Loading & Saving ---
    def load_image(self, index):
        if 0 <= index < len(self.image_list):
            # Auto save previous
            if self.current_image_index != -1 and self.auto_save.get():
                self.save_annotations()

            self.current_image_index = index
            self.file_listbox.selection_clear(0, tk.END)
            self.file_listbox.selection_set(index)
            self.file_listbox.see(index)
            
            filename = self.image_list[index]
            path = os.path.join(self.image_dir, filename)
            
            try:
                self.current_image = Image.open(path)
                
                # RESET CACHE logic when loading new image
                self.cached_dims = None
                self.cached_image_obj = None
                
                self.load_annotations(filename)
                self.redraw_canvas()
                self.update_box_list()
                self.root.title(f"AnnotationTool - {filename} [{index+1}/{len(self.image_list)}]")
            except Exception as e:
                print(f"Error loading image: {e}")

    def load_annotations(self, filename):
        self.boxes = []
        self.selected_indices = set()
        if not self.output_dir:
            return
            
        name, _ = os.path.splitext(filename)
        txt_path = os.path.join(self.output_dir, name + ".txt")
        
        if os.path.exists(txt_path):
            w, h = self.current_image.size
            self.boxes = parse_yolo(txt_path, w, h) # Returns normalized boxes

    def save_annotations(self):
        if self.current_image_index == -1 or not self.output_dir:
            return
            
        filename = self.image_list[self.current_image_index]
        name, _ = os.path.splitext(filename)
        txt_path = os.path.join(self.output_dir, name + ".txt")
        
        # Filter boxes to ensure valid classes
        valid_boxes = [b for b in self.boxes if any(c['id'] == b['class_id'] for c in self.classes) or b['class_id'] == -1]
        
        # Only save if we have boxes or file exists (to clear it)
        if valid_boxes or os.path.exists(txt_path):
             # We should probably filter out -1 (Unlabeled) before saving to YOLO?
             # Standard YOLO doesn't support -1. Let's warn or skip?
             # For now, let's skip -1 classes.
             final_boxes = [b for b in valid_boxes if b['class_id'] != -1]
             save_yolo(txt_path, final_boxes)

    # --- Canvas Drawing ---
    def redraw_canvas(self):
        if not self.current_image:
            return
            
        # Calculate ideal dimensions
        cw = self.canvas.winfo_width()
        ch = self.canvas.winfo_height()
        iw, ih = self.current_image.size
        
        if cw == 0 or ch == 0: return # Not ready yet
        
        scale_w = cw / iw
        scale_h = ch / ih
        base_scale = min(scale_w, scale_h)
        
        self.scale = base_scale * self.zoom_factor
        
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
        # If dimensions match cached dimensions, we skip resizing!
        if self.cached_dims != (nw, nh) or self.cached_image_obj is None:
            # We need to resize
            
            # Use NEAREST (fast) if we are interacting (drawing, moving, resizing, panning)
            # Use LANCZOS (quality) if idle
            is_interacting = self.is_drawing or self.resize_mode or self.move_mode or self.is_panning
            resample_method = Image.NEAREST if is_interacting else Image.LANCZOS
            
            resized = self.current_image.resize((nw, nh), resample_method)
            self.tk_image = ImageTk.PhotoImage(resized)
            self.cached_image_obj = self.tk_image
            self.cached_dims = (nw, nh)
            
            # Recreate image item
            self.canvas.delete("image_bg")
            self.canvas.create_image(self.offset_x, self.offset_y, anchor=tk.NW, image=self.tk_image, tags="image_bg")
            self.canvas.tag_lower("image_bg")
        else:
            # Dimensions match, just update position
            # If the image item doesn't exist (e.g. cleared elsewhere), recreate it
            if not self.canvas.find_withtag("image_bg"):
                 self.canvas.create_image(self.offset_x, self.offset_y, anchor=tk.NW, image=self.cached_image_obj, tags="image_bg")
                 self.canvas.tag_lower("image_bg")
            else:
                 self.canvas.coords("image_bg", self.offset_x, self.offset_y)

        # Clear only overlays (boxes, grid lines, etc) - NOT the image
        # We use strict tags to manage this
        self.canvas.delete("box")
        self.canvas.delete("handle")
        self.canvas.delete("label")
        self.canvas.delete("temp_rect")
        self.canvas.delete("grid_line")
        
        # Draw boxes
        for i, box in enumerate(self.boxes):
            self.draw_box_on_canvas(box, i in self.selected_indices, i)
            
        # Sync Right Sidebar Selection
        self.box_listbox.selection_clear(0, tk.END)
        for i in self.selected_indices:
            self.box_listbox.selection_set(i)

    def draw_box_on_canvas(self, box, is_selected, index):
        if not self.current_image: return
        
        iw, ih = self.current_image.size
        x1, y1, x2, y2 = denormalize_box(box, iw, ih)
        
        # Scale to canvas
        cx1 = x1 * self.scale + self.offset_x
        cy1 = y1 * self.scale + self.offset_y
        cx2 = x2 * self.scale + self.offset_x
        cy2 = y2 * self.scale + self.offset_y
        
        class_info = next((c for c in self.classes if c['id'] == box['class_id']), None)
        
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
        if self.show_labels.get():
            # Draw text background
            text_x = cx1
            text_y = cy1 - 15
            if text_y < 0: text_y = cy1 + 5
            
            self.canvas.create_text(text_x, text_y, text=label_text, fill=color, anchor=tk.SW, font=("Segoe UI", 9, "bold"), tags="label")

        # Draw Resize Handles if selected
        if is_selected:
            handle_size = 6
            handles = [
                (cx1, cy1, 'nw'), (cx2, cy1, 'ne'),
                (cx1, cy2, 'sw'), (cx2, cy2, 'se')
            ]
            for hx, hy, tag in handles:
                self.canvas.create_rectangle(
                    hx - handle_size/2, hy - handle_size/2,
                    hx + handle_size/2, hy + handle_size/2,
                    fill="white", outline="black", tags=("handle", f"handle_{index}_{tag}")
                )

    def on_canvas_resize(self, event):
        if self.current_image:
            self.redraw_canvas()
            
    def on_zoom(self, event):
        if not self.current_image: return
        
        if event.delta > 0:
            self.zoom_factor *= 1.1
        else:
            self.zoom_factor /= 1.1
            
        # Clamp zoom
        self.zoom_factor = max(0.1, min(self.zoom_factor, 10.0))
        
        self.redraw_canvas()
        
    def start_pan(self, event):
        self.canvas.scan_mark(event.x, event.y)
        
    def pan_image(self, event):
        self.is_panning = True # Set flag for optimized rendering
        self.canvas.scan_dragto(event.x, event.y, gain=1)
        self.redraw_canvas() # Force redraw to update overlays if needed, although scan_dragto moves the canvas content efficiently?
        # scan_dragto moves the viewport. Elements move.
        # But our custom overlay drawing might need refresh?
        # Actually standard canvas scan works on all objects.
        # BUT we are manually managing "scale" and "offset".
        # If we use scan_dragto, we are shifting the view.
        # Ideally we should implement "Manual Pan" by adjusting offset_x/y and redrawing, OR use canvas built-in.
        # The existing code used scan_mark/scan_dragto which is built-in.
        # If built-in is used, we don't necessarily need to call redraw_canvas, BUT
        # if we do `redraw_canvas` it resets the view based on offset_x/y.
        # CONFLICT: scan_dragto changes the canvas internal mapping. `redraw_canvas` resets it based on logic.
        # If we use scan_dragto, we should probably NOT call redraw_canvas OR we should update offset_x/y based on it.
        # Given the existing code... let's trust scan_dragto works for visual, but our logic might reset it.
        # We'll set is_panning = True solely for the resolution drop.
        
    def stop_pan(self, event):
        self.is_panning = False
        self.redraw_canvas() # Restore High Quality
    
    def update_cursor(self):
        """Update cursor based on current state"""
        if self.current_class_index >= 0:
            # Class is selected - show crosshair cursor
            self.canvas.config(cursor="crosshair")
        else:
            # Idle mode - show default cursor
            self.canvas.config(cursor="")
    
    def on_canvas_motion(self, event):
        """Draw grid lines following the mouse when a class is selected"""
        if not self.current_image:
            return
        
        # Only show grid lines when a class is selected (not idle)
        if self.current_class_index >= 0:
            # Adjust coordinates for scroll
            canvas_x = self.canvas.canvasx(event.x)
            canvas_y = self.canvas.canvasy(event.y)
            
            # Remove old grid lines
            self.canvas.delete("grid_line")
            
            # Get canvas dimensions
            canvas_width = self.canvas.winfo_width()
            canvas_height = self.canvas.winfo_height()
            
            # Draw vertical line
            self.canvas.create_line(
                canvas_x, 0, canvas_x, canvas_height,
                fill="#00FF00", width=1, dash=(2, 4), tags="grid_line"
            )
            
            # Draw horizontal line
            self.canvas.create_line(
                0, canvas_y, canvas_width, canvas_y,
                fill="#00FF00", width=1, dash=(2, 4), tags="grid_line"
            )
        else:
            # Remove grid lines when idle
            self.canvas.delete("grid_line")


    def on_canvas_click(self, event):
        if not self.current_image: return
        
        # Adjust coordinates for scroll
        canvas_x = self.canvas.canvasx(event.x)
        canvas_y = self.canvas.canvasy(event.y)
        
        # Check for resize handles first
        if len(self.selected_indices) == 1:
            idx = list(self.selected_indices)[0]
            # Check handles
            item = self.canvas.find_closest(canvas_x, canvas_y, halo=5)
            tags = self.canvas.gettags(item)
            for tag in tags:
                if tag.startswith(f"handle_{idx}_"):
                    self.resize_mode = True
                    self.resize_handle = tag.split('_')[-1]
                    self.resize_box_index = idx
                    self.start_x = canvas_x
                    self.start_y = canvas_y
                    return

        # Check if clicked on a box
        clicked_box_index = self.find_box_at(canvas_x, canvas_y)
        
        if clicked_box_index != -1:
            # Simple click: Select only this one
            self.selected_indices = {clicked_box_index}
            
            # Check if we should start moving (if already selected or just selected)
            # If we clicked inside a box, we prepare for move
            self.move_mode = True
            self.move_box_index = clicked_box_index
            self.start_x = canvas_x
            self.start_y = canvas_y
            
            self.redraw_canvas()
        else:
            # IDLE CHECK: If no class selected, do nothing (or clear selection)
            if self.current_class_index == -1:
                self.selected_indices = set()
                self.redraw_canvas()
                return

            # Start drawing
            self.selected_indices = set()
            self.is_drawing = True
            self.start_x = canvas_x
            self.start_y = canvas_y
            self.redraw_canvas() # Clear selection

    def on_canvas_drag(self, event):
        # Adjust coordinates for scroll
        canvas_x = self.canvas.canvasx(event.x)
        canvas_y = self.canvas.canvasy(event.y)
        
        # Clamp to image boundaries
        if self.current_image:
            iw, ih = self.current_image.size
            # Image boundaries in canvas coords
            min_x = self.offset_x
            min_y = self.offset_y
            max_x = self.offset_x + (iw * self.scale)
            max_y = self.offset_y + (ih * self.scale)
            
            canvas_x = max(min_x, min(max_x, canvas_x))
            canvas_y = max(min_y, min(max_y, canvas_y))

        if self.resize_mode:
            # Handle resizing
            box = self.boxes[self.resize_box_index]
            iw, ih = self.current_image.size
            x1, y1, x2, y2 = denormalize_box(box, iw, ih)
            
            # Convert event delta to image delta
            dx = (canvas_x - self.start_x) / self.scale
            dy = (canvas_y - self.start_y) / self.scale
            
            if self.resize_handle == 'nw':
                x1 += dx; y1 += dy
            elif self.resize_handle == 'ne':
                x2 += dx; y1 += dy
            elif self.resize_handle == 'sw':
                x1 += dx; y2 += dy
            elif self.resize_handle == 'se':
                x2 += dx; y2 += dy
                
            # Clamp to [0, iw] and [0, ih]
            x1 = max(0, min(iw, x1))
            y1 = max(0, min(ih, y1))
            x2 = max(0, min(iw, x2))
            y2 = max(0, min(ih, y2))
                
            # Normalize and update
            # Ensure x1 < x2, y1 < y2 logic handled by normalize_box
            new_box = normalize_box(x1, y1, x2, y2, iw, ih)
            new_box['class_id'] = box['class_id'] # Keep class
            
            self.boxes[self.resize_box_index] = new_box
            
            self.start_x = canvas_x
            self.start_y = canvas_y
            self.redraw_canvas()
            return

        if self.move_mode:
            # Handle moving
            box = self.boxes[self.move_box_index]
            iw, ih = self.current_image.size
            x1, y1, x2, y2 = denormalize_box(box, iw, ih)
            
            # Convert event delta to image delta
            dx = (canvas_x - self.start_x) / self.scale
            dy = (canvas_y - self.start_y) / self.scale
            
            # Update coords
            x1 += dx
            y1 += dy
            x2 += dx
            y2 += dy
            
            # Clamp to [0, iw] and [0, ih]
            # We need to check width/height to ensure we don't collapse or go out
            w = x2 - x1
            h = y2 - y1
            
            if x1 < 0: x1 = 0; x2 = w
            if y1 < 0: y1 = 0; y2 = h
            if x2 > iw: x2 = iw; x1 = iw - w
            if y2 > ih: y2 = ih; y1 = ih - h
            
            # Normalize and update
            new_box = normalize_box(x1, y1, x2, y2, iw, ih)
            new_box['class_id'] = box['class_id']
            
            self.boxes[self.move_box_index] = new_box
            
            self.start_x = canvas_x
            self.start_y = canvas_y
            self.redraw_canvas()
            return

        if self.is_drawing:
            # Update temp rectangle
            self.canvas.delete("temp_rect")
            self.canvas.create_rectangle(self.start_x, self.start_y, canvas_x, canvas_y, outline="white", dash=(4, 4), tags="temp_rect")

    def on_canvas_release(self, event):
        # Adjust coordinates for scroll
        canvas_x = self.canvas.canvasx(event.x)
        canvas_y = self.canvas.canvasy(event.y)
        
        # Clamp to image boundaries
        if self.current_image:
            iw, ih = self.current_image.size
            min_x = self.offset_x
            min_y = self.offset_y
            max_x = self.offset_x + (iw * self.scale)
            max_y = self.offset_y + (ih * self.scale)
            
            canvas_x = max(min_x, min(max_x, canvas_x))
            canvas_y = max(min_y, min(max_y, canvas_y))

        if self.resize_mode:
            self.resize_mode = False
            self.resize_handle = None
            self.resize_box_index = -1
            return

        if self.move_mode:
            self.move_mode = False
            self.move_box_index = -1
            return

        if self.is_drawing:
            self.is_drawing = False
            self.canvas.delete("temp_rect")
            
            iw, ih = self.current_image.size
            
            # Check if box is big enough (Drag operation)
            if abs(canvas_x - self.start_x) > 5 or abs(canvas_y - self.start_y) > 5:
                # Convert to normalized coords
                x1 = (self.start_x - self.offset_x) / self.scale
                y1 = (self.start_y - self.offset_y) / self.scale
                x2 = (canvas_x - self.offset_x) / self.scale
                y2 = (canvas_y - self.offset_y) / self.scale
                
                # Clamp
                x1 = max(0, min(iw, x1))
                y1 = max(0, min(ih, y1))
                x2 = max(0, min(iw, x2))
                y2 = max(0, min(ih, y2))
                
                # TEMPLATE MODE: Save size to current class
                if self.template_mode:
                    if not self.classes: return
                    
                    w = abs(x2 - x1)
                    h = abs(y2 - y1)
                    
                    # Update current class default size
                    self.classes[self.current_class_index]['default_w'] = int(w)
                    self.classes[self.current_class_index]['default_h'] = int(h)
                    
                    self.template_mode = False
                    self.update_class_list()
                    messagebox.showinfo("Template Saved", f"Updated template size for '{self.classes[self.current_class_index]['name']}'")
                    return

                new_box = normalize_box(x1, y1, x2, y2, iw, ih)
                
                # Assign class if available, else -1 (Unlabeled)
                if self.classes:
                    new_box['class_id'] = self.classes[self.current_class_index]['id']
                else:
                    new_box['class_id'] = -1

                self.boxes.append(new_box)
                self.selected_indices = {len(self.boxes) - 1}
                self.update_box_list()
                self.redraw_canvas()

            # Click operation (Stamp Template)
            else:
                if self.template_mode:
                    self.template_mode = False
                    messagebox.showinfo("Info", "Template mode cancelled.")
                    return

                if not self.classes:
                    messagebox.showwarning("Warning", "No classes loaded!")
                    return
                
                current_class = self.classes[self.current_class_index]
                default_w = current_class.get('default_w', 100)
                default_h = current_class.get('default_h', 100)
                
                # Top-Left at click
                click_x = (canvas_x - self.offset_x) / self.scale
                click_y = (canvas_y - self.offset_y) / self.scale
                
                x1 = click_x
                y1 = click_y
                x2 = click_x + default_w
                y2 = click_y + default_h
                
                # Clamp stamp to image
                if x2 > iw: x1 = iw - default_w; x2 = iw
                if y2 > ih: y1 = ih - default_h; y2 = ih
                if x1 < 0: x1 = 0
                if y1 < 0: y1 = 0
                
                new_box = normalize_box(x1, y1, x2, y2, iw, ih)
                new_box['class_id'] = current_class['id']
                
                self.boxes.append(new_box)
                self.selected_indices = {len(self.boxes) - 1}
                self.update_box_list()
                self.redraw_canvas()
                
    def find_box_at(self, x, y):
        # Reverse search to find top-most
        if not self.current_image: return -1
        
        iw, ih = self.current_image.size
        
        # Convert screen x,y to image x,y
        img_x = (x - self.offset_x) / self.scale
        img_y = (y - self.offset_y) / self.scale
        
        for i in range(len(self.boxes) - 1, -1, -1):
            box = self.boxes[i]
            bx1, by1, bx2, by2 = denormalize_box(box, iw, ih)
            
            if bx1 <= img_x <= bx2 and by1 <= img_y <= by2:
                return i
        return -1

    # --- Right Sidebar Logic ---
    def update_box_list(self):
        self.box_listbox.delete(0, tk.END)
        for i, box in enumerate(self.boxes):
            class_id = box['class_id']
            if class_id == -1:
                name = "Unlabeled"
                color = "#FFFFFF"
            else:
                class_info = next((c for c in self.classes if c['id'] == class_id), None)
                name = class_info['name'] if class_info else "Unknown"
                color = class_info['color'] if class_info else "#FFFFFF"
            
            self.box_listbox.insert(tk.END, f"{i+1}: {name}")
            self.box_listbox.itemconfig(tk.END, {'bg': color, 'fg': 'black' if self.is_light(color) else 'white'})

    def on_box_list_select(self, event):
        sel = self.box_listbox.curselection()
        self.selected_indices = set(sel)
        self.redraw_canvas()

    # --- Class Management ---
    def enter_template_mode(self):
        if not self.classes:
            messagebox.showwarning("Warning", "No classes available.")
            return
        self.template_mode = True
        messagebox.showinfo("Template Mode", f"Draw a box to define the template size for '{self.classes[self.current_class_index]['name']}'.")

    def edit_selected_box_class(self):
        if self._is_input_focused(): return
        if not self.selected_indices:
            return
        
        # Ask user to select a class
        # Since we can't easily pop up a custom listbox dialog without more code, 
        # let's use simpledialog to ask for Class ID or Name?
        # Better: Create a Toplevel window with a listbox.
        
        top = tk.Toplevel(self.root)
        top.title("Select Class")
        top.geometry("300x400")
        top.configure(bg=THEME['bg_main'])
        
        lb = DarkListbox(top)
        lb.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        
        for c in self.classes:
            lb.insert(tk.END, c['name'])
            lb.itemconfig(tk.END, {'bg': c['color'], 'fg': 'black' if self.is_light(c['color']) else 'white'})
            
        def on_select(event):
            sel = lb.curselection()
            if sel:
                new_class_id = self.classes[sel[0]]['id']
                for idx in self.selected_indices:
                    self.boxes[idx]['class_id'] = new_class_id
                self.update_box_list()
                self.redraw_canvas()
                top.destroy()
                
        lb.bind('<<ListboxSelect>>', on_select)

    # Removed add_class / remove_class as they are managed via file now
    def add_class(self):
        pass

    def remove_class(self):
        pass

    def filter_classes(self, *args):
        """Filter class list based on search text"""
        search_text = self.class_search_var.get().lower()
        if not search_text:
            self.filtered_classes = [(i, c) for i, c in enumerate(self.classes)]
        else:
            self.filtered_classes = []
            for i, c in enumerate(self.classes):
                if search_text in c['name'].lower():
                    self.filtered_classes.append((i, c))
        self.update_class_list()

    def update_class_list(self):
        self.class_listbox.delete(0, tk.END)
        for i, c in self.filtered_classes:
            text = f"{c['name']} ({c.get('default_w', 100)}x{c.get('default_h', 100)})"
            self.class_listbox.insert(tk.END, text)
            
        # Re-apply selection if visible
        if self.current_class_index != -1:
            # Find if current class is in filtered list
            for list_idx, (real_idx, _) in enumerate(self.filtered_classes):
                if real_idx == self.current_class_index:
                    self.class_listbox.selection_clear(0, tk.END)
                    self.class_listbox.selection_set(list_idx)
                    self.class_listbox.see(list_idx)
                    break

    def on_class_select(self, event):
        sel = self.class_listbox.curselection()
        if sel:
            # Map listbox index to real class index
            list_index = sel[0]
            if list_index < len(self.filtered_classes):
                self.current_class_index = self.filtered_classes[list_index][0]
                self.update_cursor()

    def cycle_class(self):
        if self._is_input_focused(): return
        if not self.classes: return
        self.current_class_index = (self.current_class_index + 1) % len(self.classes)
        self.class_listbox.selection_clear(0, tk.END)
        self.class_listbox.selection_set(self.current_class_index)
        self.class_listbox.see(self.current_class_index)

    def is_light(self, hex_color):
        h = hex_color.lstrip('#')
        r, g, b = tuple(int(h[i:i+2], 16) for i in (0, 2, 4))
        return (r*0.299 + g*0.587 + b*0.114) > 186

    # --- Navigation ---
    def next_image(self):
        if self._is_input_focused(): return
        if self.image_list:
            new_idx = (self.current_image_index + 1) % len(self.image_list)
            self.load_image(new_idx)

    def prev_image(self):
        if self._is_input_focused(): return
        if self.image_list:
            new_idx = (self.current_image_index - 1) % len(self.image_list)
            self.load_image(new_idx)
            
    def on_file_select(self, event):
        sel = self.file_listbox.curselection()
        if sel:
            self.load_image(sel[0])

    # --- Box Operations ---
    def delete_selected_box(self):
        if self._is_input_focused(): return
        if self.selected_indices:
            # Delete in reverse order to avoid index shifting issues
            for idx in sorted(self.selected_indices, reverse=True):
                del self.boxes[idx]
            
            self.selected_indices = set()
            self.update_box_list()
            self.redraw_canvas()

    def copy_boxes(self):
        if self._is_input_focused(): return
        if self.selected_indices:
            self.clipboard = [self.boxes[i].copy() for i in self.selected_indices]
        else:
            # Copy all if none selected? Or nothing?
            # Let's copy all if none selected, as per original logic, or maybe just nothing?
            # Original logic: "Copy selected box (or all if none selected)"
            self.clipboard = [b.copy() for b in self.boxes]
            
        messagebox.showinfo("Info", f"Copied {len(self.clipboard)} boxes.")

    def paste_boxes(self):
        if self._is_input_focused(): return
        if not self.clipboard: return
        
        for box in self.clipboard:
            self.boxes.append(box.copy())
        
        self.update_box_list()
        self.redraw_canvas()

