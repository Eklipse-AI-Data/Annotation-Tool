import os
import sys
import time
import shutil
import threading
import concurrent.futures
import tkinter as tk
from tkinter import filedialog, messagebox, ttk
from PIL import Image
from src.utils import (natural_sort_key, save_config, save_classes, load_classes,
                       create_class_mapping, update_annotation_file, backup_annotations,
                       normalize_box, denormalize_box, get_recent_projects,
                       resize_images_to_lowres, ThumbnailLoader,
                       CONFIG_FILE, CLASSES_FILE, DATA_DIR)
from src.ui_components import DarkFrame, DarkLabel, DarkButton, DarkEntry, ProgressDialog, THEME

class AnnotationController:
    def __init__(self, model, view):
        self.model = model
        self.view = view
        self.preset_hold_delay_ms = 250
        self.pending_preset_preview_after_id = None
        self.pending_preset_slot = None
        self.pending_preset_key = None
        self.pending_preset_preview_shown = False
        self.preset_preview_lock = False
        
        self._task_running = False
        self.redraw_timer = None
        self._pending_zoom = 1.0
        self._zoom_anchor = None
        self._zoom_job = None
        self.autosave_timer_id = None
        self.thumb_loader = ThumbnailLoader()
        self.view.thumb_strip.on_click = self.on_thumb_click
        self.view.thumb_strip.request_thumbs = self.request_thumbnails
        self.bind_events()
        self.init_state()
        
        # Start auto-save timer for crash recovery
        self.start_autosave_timer()

    def _ui_call(self, fn):
        """Marshal fn onto the UI thread; safe against app shutdown races."""
        try:
            self.view.root.after(0, fn)
        except (RuntimeError, tk.TclError):
            pass

    def run_threaded_task(self, title, work_fn, on_done):
        """Run work_fn on a background thread with a modal progress dialog.

        work_fn(report) runs OFF the UI thread and must never touch Tk
        widgets. report(current, total, text) and on_done(result) are
        marshaled back to the UI thread. If work_fn raises, on_done
        receives the exception as its argument.
        """
        if self._task_running:
            return
        self._task_running = True
        dialog = ProgressDialog(self.view.root, title=title)
        last_update = [0.0]

        def report(current, total, text=""):
            # Throttle UI updates so huge directories don't flood the event loop
            now = time.monotonic()
            if now - last_update[0] < 0.05 and current != total:
                return
            last_update[0] = now
            self._ui_call(lambda: dialog.update_progress(current, total, text))

        def worker():
            try:
                result = work_fn(report)
            except Exception as e:
                result = e

            def finish():
                self._task_running = False
                dialog.close()
                on_done(result)
            self._ui_call(finish)

        threading.Thread(target=worker, daemon=True).start()

    def is_focus_on_entry(self):
        """Check if focus is currently on an Entry widget (e.g., search box)"""
        focused = self.view.root.focus_get()
        return isinstance(focused, (tk.Entry, ttk.Entry))

    def start_autosave_timer(self):
        """Start the auto-save timer for crash recovery (runs every 30 seconds)."""
        self.autosave_session()
        # Schedule next autosave
        self.autosave_timer_id = self.view.root.after(30000, self.start_autosave_timer)
    
    def autosave_session(self):
        """Auto-save current state for crash recovery."""
        try:
            if self.model.auto_save.get() and self.model.current_image_index != -1:
                self.model.save_annotations()
            self.model.save_session()
        except Exception as e:
            print(f"Autosave error: {e}")

    def init_state(self):
        # Update UI with initial model state
        self.update_class_list()
        self.update_filter_combo()
        self.update_recent_projects_menu()
        
        # Verify session directories exist before loading
        if self.model.image_dir and not os.path.isdir(self.model.image_dir):
            messagebox.showwarning(
                "Session Warning",
                f"The last image directory no longer exists:\n{self.model.image_dir}\n\nPlease select a new directory."
            )
            self.model.image_dir = ''
            self.model.current_image_index = -1
            
        if self.model.output_dir and not os.path.isdir(self.model.output_dir):
            messagebox.showwarning(
                "Session Warning", 
                f"The last output directory no longer exists:\n{self.model.output_dir}\n\nPlease select a new output directory."
            )
            self.model.output_dir = ''
        
        self.view.update_dir_label(self.model.image_dir)
        
        if self.model.image_dir:
            self.load_images()
            if self.model.image_list:
                index = self.model.current_image_index
                if 0 <= index < len(self.model.image_list):
                    self.load_image(index)
                else:
                    self.load_image(0)
            self.refresh_class_counts()
        
        # Initial sidebar state
        if not self.model.show_right_sidebar.get():
            self.view.toggle_right_sidebar(False)
            
        if not self.model.show_left_sidebar.get():
            self.view.toggle_left_sidebar(False)

        if not self.model.show_filmstrip.get():
            self.view.toggle_filmstrip(False)
            
        # Apply theme from session
        from src.themes import PREDEFINED_THEMES
        theme_name = self.model.current_theme_name
        if theme_name in PREDEFINED_THEMES:
            self.view.apply_theme(PREDEFINED_THEMES[theme_name])

    def refresh_preset_scope(self):
        self.model.load_scoped_presets()
        self.clear_preview(redraw=False)
        if hasattr(self, 'box_presets_lb') and self.box_presets_lb.winfo_exists():
            self.update_box_presets_list()

    def clear_preview(self, redraw=True):
        if not self.model.preview_visible and not self.model.preview_boxes:
            return False

        self.model.clear_preview()
        if redraw and self.model.current_image_pil:
            self.view.redraw_canvas()
        return True

    def toggle_clipboard_preview(self):
        if self._is_input_focused():
            return
        if not self.model.current_image_pil:
            self.view.update_status("Load an image to preview the clipboard")
            return
        if self.model.preview_visible and self.model.preview_source == 'clipboard':
            self.clear_preview()
            self.view.update_status("Clipboard preview hidden")
            return
        if not self.model.clipboard:
            self.view.update_status("Clipboard is empty")
            return

        self.model.set_preview(self.model.clipboard, 'clipboard')
        self.view.redraw_canvas()
        self.view.update_status(f"Previewing clipboard: {len(self.model.preview_boxes)} box(es)")

    def preview_preset(self, slot, toggle=True):
        if not self.model.current_image_pil:
            self.view.update_status("Load an image to preview a preset")
            return

        preset_boxes = self.model.presets.get(slot, [])
        if not preset_boxes:
            self.view.update_status(f"Preset {slot} is empty")
            return

        if toggle and self.model.preview_visible and self.model.preview_source == 'preset' and self.model.preview_slot == slot:
            self.clear_preview()
            self.view.update_status(f"Preset {slot} preview hidden")
            return

        self.model.set_preview(preset_boxes, 'preset', slot=slot)
        self.view.redraw_canvas()
        self.view.update_status(f"Previewing Preset {slot}: {len(self.model.preview_boxes)} box(es)")

    def preview_selected_slot(self):
        sel = self.box_presets_lb.curselection()
        if not sel:
            messagebox.showwarning("Warning", "Please select a slot in the list first.")
            return
        self.preview_preset(sel[0] + 1)

    def _cancel_pending_preset_hold(self, reset_preview_lock=False):
        if self.pending_preset_preview_after_id:
            self.view.root.after_cancel(self.pending_preset_preview_after_id)
        self.pending_preset_preview_after_id = None
        self.pending_preset_slot = None
        self.pending_preset_key = None
        self.pending_preset_preview_shown = False
        if reset_preview_lock:
            self.preset_preview_lock = False

    def _begin_preset_hold(self, slot, key_name):
        if self.is_focus_on_entry():
            return

        if self.preset_preview_lock:
            return

        if self.pending_preset_slot == slot and self.pending_preset_key == key_name:
            return

        self._cancel_pending_preset_hold()

        self.pending_preset_slot = slot
        self.pending_preset_key = key_name

        def show_preview():
            self.pending_preset_preview_after_id = None
            self.pending_preset_preview_shown = True
            self.preset_preview_lock = True
            self.preview_preset(slot, toggle=False)

        self.pending_preset_preview_after_id = self.view.root.after(self.preset_hold_delay_ms, show_preview)

    def _finish_preset_hold(self, slot, key_name, release_control=False):
        if self.pending_preset_slot != slot or self.pending_preset_key != key_name:
            return

        if self.pending_preset_preview_shown:
            if release_control and self.model.preview_visible and self.model.preview_source == 'preset' and self.model.preview_slot == slot:
                self.clear_preview()
                self.view.update_status(f"Preset {slot} preview hidden")
            self._cancel_pending_preset_hold(reset_preview_lock=release_control)
            return

        self._cancel_pending_preset_hold()

        self.apply_preset(slot)

    def _on_control_release(self, event):
        if self.pending_preset_slot is None:
            if self.preset_preview_lock and self.model.preview_visible and self.model.preview_source == 'preset':
                slot = self.model.preview_slot
                self.clear_preview()
                if slot is not None:
                    self.view.update_status(f"Preset {slot} preview hidden")
            self.preset_preview_lock = False
            return

        slot = self.pending_preset_slot
        key_name = self.pending_preset_key
        self._finish_preset_hold(slot, key_name, release_control=True)

    def _bind_preset_hold_shortcuts(self, slot, press_pattern, release_pattern, key_name):
        root = self.view.root
        root.bind(press_pattern, lambda e, num=slot, key=key_name: self._begin_preset_hold(num, key) if not (e.state & 0x0001) else None)
        root.bind(release_pattern, lambda e, num=slot, key=key_name: self._finish_preset_hold(num, key, release_control=False))

    def bind_events(self):
        # UI Button Commands
        self.view.open_images_btn.configure(command=self.select_image_dir)
        self.view.set_output_btn.configure(command=self.select_output_dir)
        self.view.toggle_left_btn.configure(command=self.toggle_left_sidebar)
        self.view.toggle_sidebar_btn.configure(command=self.toggle_right_sidebar)
        self.view.undo_btn.configure(command=self.undo)
        self.view.redo_btn.configure(command=self.redo)
        self.view.settings_btn.configure(command=self.open_settings_dialog)
        self.view.filter_btn.configure(command=self.apply_image_filter)
        self.view.clear_filter_btn.configure(command=self.clear_image_filter)
        self.view.copy_btn.configure(command=self.copy_boxes)
        self.view.paste_btn.configure(command=self.paste_boxes)
        self.view.preview_clipboard_btn.configure(command=self.toggle_clipboard_preview)
        self.view.duplicate_btn.configure(command=self.duplicate_boxes)
        self.view.validate_btn.configure(command=self.show_validation_dialog)
        self.view.shortcut_help_btn.configure(command=self.toggle_shortcut_overlay)
        self.view.load_recent_btn.configure(command=self.load_recent_project)
        self.view.toggle_filmstrip_btn.configure(command=self.toggle_filmstrip)
        
        # List Binds (Treeview-based)
        self.view.class_listbox.bind('<<TreeviewSelect>>', self.on_class_select)
        self.view.file_listbox.bind('<<TreeviewSelect>>', self.on_file_select)
        self.view.box_listbox.bind('<<TreeviewSelect>>', self.on_box_list_select)
        
        # Canvas Binds
        self.view.canvas.bind('<Configure>', self.on_canvas_configure)
        self.view.canvas.bind('<Button-1>', self.on_canvas_click)
        self.view.canvas.bind('<B1-Motion>', self.on_canvas_drag)
        self.view.canvas.bind('<ButtonRelease-1>', self.on_canvas_release)
        self.view.canvas.bind('<Motion>', self.on_canvas_motion)
        
        # Make canvas focusable so shortcuts work after clicking on it
        self.view.canvas.configure(takefocus=True)
        
        # Panning and Zooming
        self.view.canvas.bind('<Button-3>', self.start_pan)
        self.view.canvas.bind('<B3-Motion>', self.on_pan)
        self.view.canvas.bind('<MouseWheel>', self.on_zoom)
        
        # Keyboard Shortcuts (using config)
        # Wrapper to only trigger shortcuts when not typing in an Entry widget
        def shortcut(action):
            def handler(e):
                if not self.is_focus_on_entry():
                    return action()
            return handler
        
        root = self.view.root
        cfg = self.model.config
        root.bind(cfg['prev_image'], shortcut(self.prev_image))
        root.bind(cfg['next_image'], shortcut(self.next_image))
        root.bind(cfg['delete_box'], shortcut(self.delete_selected_box))
        root.bind(cfg['copy'], shortcut(self.copy_boxes))
        root.bind(cfg['paste'], shortcut(self.paste_boxes))
        root.bind(cfg['duplicate'], shortcut(self.duplicate_boxes))
        root.bind(cfg['cycle_class'], shortcut(self.cycle_class))
        root.bind(cfg['edit_class'], shortcut(self.edit_selected_box_class))
        root.bind(cfg['deselect'], lambda e: self.deselect_class())  # Escape always works (to leave entry)
        root.bind(cfg['undo'], shortcut(self.undo))
        root.bind(cfg['redo'], shortcut(self.redo))
        root.bind("<KeyRelease-Control_L>", self._on_control_release)
        root.bind("<KeyRelease-Control_R>", self._on_control_release)
        
        # Secondary Navigation
        root.bind("<Left>", shortcut(self.prev_image))
        root.bind("<Right>", shortcut(self.next_image))
        
        # Bounding Box Presets shortcuts - robust handling for various layouts and Numpad
        for i in range(1, 10):
            # Standard number keys: tap to apply, hold to preview
            self._bind_preset_hold_shortcuts(i, f"<Control-Key-{i}>", f"<KeyRelease-{i}>", f"digit_{i}")
            root.bind(f"<Control-Shift-Key-{i}>", lambda e, num=i: self.save_preset(num) if not self.is_focus_on_entry() else None)
            # Numpad compatibility
            self._bind_preset_hold_shortcuts(i, f"<Control-KP_{i}>", f"<KeyRelease-KP_{i}>", f"kp_{i}")
            root.bind(f"<Control-Shift-KP_{i}>", lambda e, num=i: self.save_preset(num) if not self.is_focus_on_entry() else None)
            
        # Common shifted symbols for Ctrl+Shift+Number (specifically helps on Windows/Windows Layouts)
        shift_symbols = {
            "exclam": 1, "at": 2, "numbersign": 3, "dollar": 4, "percent": 5,
            "asciicircum": 6, "ampersand": 7, "asterisk": 8, "parenleft": 9
        }
        for sym, num in shift_symbols.items():
            root.bind(f"<Control-{sym}>", lambda e, n=num: self.save_preset(n) if not self.is_focus_on_entry() else None)
        
        # U key for unannotated filter toggle
        root.bind("<u>", shortcut(self.toggle_unannotated_filter_key))

        # Zoom keys + fit-to-window
        zoom_binds = [("<Control-plus>", self.zoom_in), ("<Control-equal>", self.zoom_in),
                      ("<Control-minus>", self.zoom_out), ("<Control-Key-0>", self.zoom_reset)]
        if sys.platform == 'darwin':
            zoom_binds += [("<Command-plus>", self.zoom_in), ("<Command-equal>", self.zoom_in),
                           ("<Command-minus>", self.zoom_out), ("<Command-Key-0>", self.zoom_reset)]
        for seq, fn in zoom_binds:
            root.bind(seq, shortcut(fn))
        
        # ? key for shortcut overlay (both ? and / work)
        root.bind("<question>", lambda e: self.toggle_shortcut_overlay() if not self.is_focus_on_entry() else None)
        root.bind("<slash>", lambda e: self.toggle_shortcut_overlay() if not self.is_focus_on_entry() and (e.state & 0x0001) else None)
        
        # Search Bar Trace
        self.view.class_search_var.trace("w", self.filter_classes)
        
        # Window Close
        root.protocol("WM_DELETE_WINDOW", self.on_close)

    def select_image_dir(self):
        path = filedialog.askdirectory(title="Select Image Directory")
        if not path:
            return

        # Save current annotations before switching directories
        if self.model.current_image_index != -1 and self.model.auto_save.get():
            self.model.save_annotations()

        # Ask user if they want to lower resolution
        response = messagebox.askyesno(
            "Lower Resolution?",
            "Do you want to lower the resolution of images before loading?\n\n"
            "This will create a new directory with '_lowres' suffix containing "
            "images resized to 720px wide (aspect ratio preserved).\n\n"
            "Original images will remain untouched."
        )

        if response:
            def work(report):
                return resize_images_to_lowres(path, progress_callback=report)

            def done(result):
                if isinstance(result, Exception) or not result:
                    messagebox.showerror("Error", "Failed to process images. Loading original directory instead.")
                    self._finish_open_dir(path)
                else:
                    self._finish_open_dir(result)

            self.run_threaded_task("Resizing Images", work, done)
        else:
            self._finish_open_dir(path)

    def _finish_open_dir(self, image_dir):
        """Continuation of select_image_dir after the (optional) resize step."""
        # Reset state so the stale image's boxes can't be auto-saved into
        # the new directory when load_image runs.
        self.model.current_image_index = -1
        self.model.boxes = []
        self.model.selected_indices = set()

        self.model.image_dir = image_dir
        # Thumbnails are keyed by filename; a new directory invalidates them
        self.thumb_loader.clear()
        self.view.thumb_strip.clear_thumbnails()
        if not self.model.output_dir:
            self.model.output_dir = image_dir

        self.refresh_preset_scope()

        self.view.update_dir_label(self.model.image_dir)
        self.load_images()
        self.load_image(0)

        # Add to recent projects
        self.model.add_to_recent_projects()
        self.update_recent_projects_menu()

        self.refresh_class_counts()
        self.model.save_session()

    def select_output_dir(self):
        directory = filedialog.askdirectory()
        if directory:
            self.model.output_dir = directory
            self.refresh_preset_scope()
            # Reload annotations for current image if one is loaded
            if self.model.image_list and self.model.current_image_index != -1:
                filename = self.model.image_list[self.model.current_image_index]
                self.model.load_annotations(filename)
                self.view.redraw_canvas()
            
            self.update_progress_bar()
            self.refresh_class_counts()
            self.model.save_session()
            messagebox.showinfo("Info", f"Output directory set to: {directory}")

    def load_images(self):
        if not self.model.image_dir: return
        files = [f for f in os.listdir(self.model.image_dir) 
                 if f.lower().endswith(('.png', '.jpg', '.jpeg'))]
        self.model.full_image_list = sorted(files, key=natural_sort_key)
        self.model.image_list = list(self.model.full_image_list)

        self._refresh_file_list()

    def _save_current_before_list_change(self):
        """Save the current image's annotations BEFORE image_list is mutated.

        load_image's auto-save resolves the filename via image_list[index];
        if the list has already been swapped (filters), it would write the
        old image's boxes under the wrong filename — silent data loss.
        """
        if self.model.current_image_index != -1 and self.model.auto_save.get():
            self.model.save_annotations()
        self.model.current_image_index = -1

    def _refresh_file_list(self):
        """Repopulate the file list and filmstrip from the current image list."""
        self.view.rebuild_file_list(self.model.image_list)
        self.view.thumb_strip.set_items(self.model.image_list)
        self.update_progress_bar()

    def load_image(self, index):
        if not self.model.image_list: return
        if 0 <= index < len(self.model.image_list):
            # Auto save previous image annotations
            if self.model.current_image_index != -1 and self.model.auto_save.get():
                self.model.save_annotations()

            self.clear_preview(redraw=False)

            self.model.current_image_index = index
            filename = self.model.image_list[index]
            path = os.path.join(self.model.image_dir, filename)
            
            try:
                # Try to get from cache first
                cached_image = self.model.image_preloader.get(path)
                if cached_image:
                    self.model.current_image_pil = cached_image
                else:
                    self.model.current_image_pil = Image.open(path)
                    self.model.current_image_pil.load()  # Force load into memory
                    self.model.image_preloader.put(path, self.model.current_image_pil)
                
                # Reset view cache
                self.view.cached_dims = None
                self.view.cached_image_obj = None
                
                # Load annotations for new image
                self.model.load_annotations(filename)
                
                # Update title
                self.view.root.title(f"Annotation Tool - {filename}")
                
                # Redraw
                self.view.redraw_canvas()
                self.view.update_box_list()
                
                # Preload adjacent images in background
                self.preload_adjacent_images()
                
                # Update validation indicator (lightweight check)
                warnings = self.model.validate_current_annotations()
                self.view.update_warning_indicator(warnings)
                
            except Exception as e:
                print(f"Error loading image: {e}")
                self.view.update_status(f"Failed to load {filename}: {e}")

            # Sync file list and filmstrip highlight with the current image
            self.view.set_file_selection(filename)
            self.view.thumb_strip.set_current(filename)

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

    def toggle_filmstrip(self):
        current = self.model.show_filmstrip.get()
        self.model.show_filmstrip.set(not current)
        self.view.toggle_filmstrip(not current)

    # ==================== THUMBNAIL FILMSTRIP ====================

    def on_thumb_click(self, filename):
        try:
            index = self.model.image_list.index(filename)
        except ValueError:
            return
        if index != self.model.current_image_index:
            self.load_image(index)

    def request_thumbnails(self, filenames):
        """Called by the strip for visible slots that have no thumbnail yet."""
        image_dir = self.model.image_dir
        if not image_dir:
            return
        for fname in filenames:
            path = os.path.join(image_dir, fname)
            # Key by full path so in-flight loads from a previous directory
            # can't shadow same-named files in the new one.
            callback = (lambda key, img, fname=fname, d=image_dir:
                        self._deliver_thumbnail(d, fname, img))
            self.thumb_loader.request(path, path, callback)

    def _deliver_thumbnail(self, image_dir, filename, pil_img):
        # May be called from a worker thread; marshal to the UI thread where
        # PhotoImage construction is safe.
        if image_dir != self.model.image_dir:
            return  # stale delivery from a directory we already left
        self._ui_call(lambda: self._set_thumbnail_photo(filename, pil_img))

    def _set_thumbnail_photo(self, filename, pil_img):
        from PIL import ImageTk
        try:
            photo = ImageTk.PhotoImage(pil_img)
        except tk.TclError:
            return  # app is shutting down
        self.view.thumb_strip.set_thumbnail(filename, photo)

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
        top.transient(self.view.root)
        top.grab_set()
        
        DarkLabel(top, text="SELECT CLASS", font=(THEME['font_family_sans'], 10, "bold")).pack(pady=10)
        
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
        preview_cleared = self.clear_preview(redraw=False)
        self.model.current_class_index = -1
        self.update_class_list()
        if preview_cleared:
            if self.model.current_image_pil:
                self.view.redraw_canvas()
            self.view.update_status("Preview cleared")

    def toggle_unannotated_filter_key(self):
        current = self.model.unannotated_filter_active.get()
        self.model.unannotated_filter_active.set(not current)
        self.toggle_unannotated_filter()

    def toggle_unannotated_filter(self):
        if getattr(self, '_filter_scan_running', False):
            # A scan is in flight; revert the checkbox toggle and ignore
            self.model.unannotated_filter_active.set(not self.model.unannotated_filter_active.get())
            return

        if self.model.unannotated_filter_active.get():
            if not self.model.output_dir:
                messagebox.showwarning("Warning", "Please set Output Directory first.")
                self.model.unannotated_filter_active.set(False)
                return

            output_dir = self.model.output_dir
            images = list(self.model.full_image_list)
            self._filter_scan_running = True
            self.view.root.config(cursor="wait")

            def scan_thread():
                def check_file(img):
                    name, _ = os.path.splitext(img)
                    txt_path = os.path.join(output_dir, name + ".txt")
                    try:
                        if not os.path.exists(txt_path) or os.path.getsize(txt_path) == 0:
                            return img
                    except OSError:
                        return img
                    return None

                with concurrent.futures.ThreadPoolExecutor() as executor:
                    results = list(executor.map(check_file, images))
                unannotated = [r for r in results if r is not None]
                self._ui_call(lambda: self._finish_unannotated_filter(unannotated))

            threading.Thread(target=scan_thread, daemon=True).start()
        else:
            self._save_current_before_list_change()
            self.model.image_list = list(self.model.full_image_list)
            self._refresh_file_list()
            if self.model.image_list:
                self.load_image(0)

    def _finish_unannotated_filter(self, unannotated):
        self._filter_scan_running = False
        self.view.root.config(cursor="")
        self._save_current_before_list_change()
        self.model.image_list = unannotated
        self._refresh_file_list()
        if self.model.image_list:
            self.load_image(0)

    def filter_classes(self, *args):
        search_term = self.view.class_search_var.get().lower()
        self.model.filtered_classes = [
            (i, c) for i, c in enumerate(self.model.classes)
            if search_term in c['name'].lower()
        ]
        self.update_class_list()

    def update_class_list(self):
        # Rebuild only when the class set itself changed; filtering and
        # selection use detach/reattach so scroll position survives.
        signature = [(c['id'], c['name'], c['color']) for c in self.model.classes]
        if self.view._class_sig != signature:
            self.view.rebuild_class_list(self.model.classes, self.model.class_counts)

        visible_ids = {c['id'] for _, c in self.model.filtered_classes}
        self.view.filter_class_list(visible_ids)

        current_id = None
        if 0 <= self.model.current_class_index < len(self.model.classes):
            current_id = self.model.classes[self.model.current_class_index]['id']
        self.view.set_class_selection(current_id)

    def refresh_class_counts(self):
        """Recount class usage across the output dir on a background thread."""
        output_dir = self.model.output_dir
        if not output_dir or not os.path.isdir(output_dir):
            return

        def scan():
            counts = {}
            try:
                txt_files = [f for f in os.listdir(output_dir) if f.lower().endswith('.txt')]
            except OSError:
                return
            for f in txt_files:
                try:
                    with open(os.path.join(output_dir, f), 'r') as fh:
                        for line in fh:
                            parts = line.split()
                            if parts:
                                cid = int(parts[0])
                                counts[cid] = counts.get(cid, 0) + 1
                except (ValueError, OSError):
                    continue
            self._ui_call(lambda: self._finish_class_counts(counts))

        threading.Thread(target=scan, daemon=True).start()

    def _finish_class_counts(self, counts):
        self.model.class_counts = counts
        self.view.update_class_counts(self.model.classes, counts)

    def update_filter_combo(self):
        values = [f"{c['id']}: {c['name']}" for c in self.model.classes]
        self.view.filter_combo['values'] = values

    def on_class_select(self, event):
        if self.view._suppress_list_events:
            return
        selection = self.view.class_listbox.tree.selection()
        if selection:
            class_id = int(selection[0])
            for i, c in enumerate(self.model.classes):
                if c['id'] == class_id:
                    self.model.current_class_index = i
                    break

    def on_file_select(self, event):
        if self.view._suppress_list_events:
            return
        selection = self.view.file_listbox.tree.selection()
        if selection:
            filename = selection[0]
            try:
                index = self.model.image_list.index(filename)
            except ValueError:
                return
            if index != self.model.current_image_index:
                self.load_image(index)

    def on_box_list_select(self, event):
        if self.view._suppress_list_events:
            return
        selected = {
            int(iid) for iid in self.view.box_listbox.tree.selection()
            if 0 <= int(iid) < len(self.model.boxes)
        }
        if selected == self.model.selected_indices:
            return
        self.model.selected_indices = selected
        self.view.redraw_canvas()

    def refresh_class_dependent_views(self):
        self.model.filtered_classes = [(i, c) for i, c in enumerate(self.model.classes)]
        if not self.model.classes:
            self.model.current_class_index = -1
        elif self.model.current_class_index >= len(self.model.classes):
            self.model.current_class_index = len(self.model.classes) - 1
        self.filter_classes()
        self.update_filter_combo()
        self.refresh_batch_listboxes()

    def update_progress_bar(self):
        count, total = self.model.get_annotated_count()
        percent = int((count / total) * 100) if total > 0 else 0
        text = f"{count}/{total} ({percent}%)"
        self.view.progress_bar.set_value(count, total, text)

    def on_close(self):
        # Cancel auto-save timer
        if self.autosave_timer_id:
            self.view.root.after_cancel(self.autosave_timer_id)
        
        # Save current image if auto_save is on
        if self.model.current_image_index != -1 and self.model.auto_save.get():
            self.model.save_annotations()

        self.model.save_session()
        self.model.image_preloader.shutdown()
        self.thumb_loader.shutdown()
        self.view.root.destroy()

    def toggle_shortcut_overlay(self):
        """Toggle the keyboard shortcut overlay."""
        self.view.toggle_shortcut_overlay()
    
    def show_validation_dialog(self):
        """Show validation results in a dialog."""
        warnings = self.model.validate_current_annotations()
        self.view.update_warning_indicator(warnings)
        
        if not warnings:
            messagebox.showinfo("Validation", "✓ All annotations are valid!")
            return
        
        # Create validation results dialog
        top = tk.Toplevel(self.view.root)
        top.title("Annotation Validation")
        top.geometry("500x400")
        top.configure(bg=THEME['bg_main'])
        # Transient only (no grab): selecting a warning highlights the box
        # on the canvas, so the main window must stay interactive.
        top.transient(self.view.root)
        
        DarkLabel(top, text="Validation Results", font=(THEME['font_family_sans'], 14, "bold")).pack(pady=10)
        
        # Summary
        error_count = sum(1 for w in warnings if w['severity'] == 'error')
        warn_count = sum(1 for w in warnings if w['severity'] == 'warning')
        info_count = sum(1 for w in warnings if w['severity'] == 'info')
        
        summary = f"Found: {error_count} errors, {warn_count} warnings, {info_count} info"
        DarkLabel(top, text=summary, fg=THEME['button_highlight']).pack(pady=5)
        
        # Warnings list
        list_frame = DarkFrame(top)
        list_frame.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        
        scrollbar = tk.Scrollbar(list_frame)
        scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
        
        listbox = tk.Listbox(list_frame, bg=THEME['list_bg'], fg=THEME['fg_text'],
                            selectbackground=THEME['selection'], relief=tk.FLAT,
                            font=(THEME['font_family_sans'], 10))
        listbox.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        listbox.config(yscrollcommand=scrollbar.set)
        scrollbar.config(command=listbox.yview)

        for i, w in enumerate(warnings):
            icon = {'error': '❌', 'warning': '⚠', 'info': 'ℹ'}.get(w['severity'], '•')
            listbox.insert(tk.END, f" {icon} {w['message']}")
        
        # Select box on click
        def on_select(event):
            sel = listbox.curselection()
            if sel and sel[0] < len(warnings):
                box_idx = warnings[sel[0]]['box_index']
                self.model.selected_indices = {box_idx}
                self.view.redraw_canvas()
                self.view.update_box_list()
        
        listbox.bind('<<ListboxSelect>>', on_select)
        
        DarkButton(top, text="Close", command=top.destroy).pack(pady=10)
    
    def preload_adjacent_images(self):
        """Preload images adjacent to current image for faster navigation."""
        if not self.model.image_list or self.model.current_image_index == -1:
            return
        
        paths_to_preload = []
        current_idx = self.model.current_image_index

        # Preload next 3 and previous 2 images (and pre-render the current
        # one so revisits at this window size skip the resize)
        for offset in [1, 2, 3, -1, -2, 0]:
            idx = current_idx + offset
            if 0 <= idx < len(self.model.image_list):
                img_path = os.path.join(self.model.image_dir, self.model.image_list[idx])
                paths_to_preload.append(img_path)

        cw = self.view.canvas.winfo_width()
        ch = self.view.canvas.winfo_height()
        canvas_size = (cw, ch) if cw > 1 and ch > 1 else None
        self.model.image_preloader.preload(paths_to_preload, canvas_size)

    def update_recent_projects_menu(self):
        """Update the recent projects dropdown menu."""
        self.model.recent_projects = get_recent_projects()
        
        if not self.model.recent_projects:
            self.view.recent_projects_combo['values'] = ["No recent projects"]
            return
        
        # Format: "ProjectName (image_dir)"
        display_values = []
        for p in self.model.recent_projects:
            name = p.get('name', 'Unknown')
            # Truncate path if too long
            img_dir = p.get('image_dir', '')
            if len(img_dir) > 40:
                img_dir = "..." + img_dir[-37:]
            display_values.append(f"{name}")
        
        self.view.recent_projects_combo['values'] = display_values
        if display_values:
            self.view.recent_projects_combo.set(display_values[0])
    
    def load_recent_project(self):
        """Load a project from the recent projects list."""
        selection_idx = self.view.recent_projects_combo.current()
        
        if selection_idx < 0 or selection_idx >= len(self.model.recent_projects):
            messagebox.showwarning("Warning", "Please select a valid project.")
            return
        
        project = self.model.recent_projects[selection_idx]
        image_dir = project.get('image_dir', '')
        output_dir = project.get('output_dir', '')
        
        # Validate directories exist
        if not image_dir or not os.path.isdir(image_dir):
            messagebox.showerror("Error", f"Image directory no longer exists:\n{image_dir}")
            return
        
        # Save current annotations before switching
        if self.model.current_image_index != -1 and self.model.auto_save.get():
            self.model.save_annotations()
        
        # Reset state
        self.model.current_image_index = -1
        self.model.boxes = []
        self.model.selected_indices = set()
        self.model.image_preloader.clear()  # Clear cache for new project
        self.thumb_loader.clear()
        self.view.thumb_strip.clear_thumbnails()

        # Load new project
        self.model.image_dir = image_dir
        self.model.output_dir = output_dir if output_dir and os.path.isdir(output_dir) else image_dir
        self.refresh_preset_scope()
        
        self.view.update_dir_label(self.model.image_dir)
        self.load_images()
        
        if self.model.image_list:
            self.load_image(0)
        
        self.model.save_session()
        self.view.update_status(f"Loaded project: {project.get('name', 'Unknown')}")

    # ==================== SETTINGS DIALOG ====================

    def open_settings_dialog(self):
        top = tk.Toplevel(self.view.root)
        top.title("Settings")
        top.geometry("600x600")
        top.configure(bg=THEME['bg_main'])
        top.transient(self.view.root)
        top.grab_set()
        
        # Store reference to the settings window
        self.settings_window = top
        
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
        self.setup_keybindings_tab(keybindings_tab)
        self.setup_class_management_tab(class_mgmt_tab)
        self.setup_batch_operations_tab(batch_ops_tab)
        self.setup_game_presets_tab(game_presets_tab)
        self.setup_box_presets_tab(box_presets_tab)
        self.setup_themes_tab(themes_tab)
        
        # Global Save & Close button at the bottom (visible from all tabs)
        save_btn = DarkButton(top, text="Save & Close", command=lambda: self.save_settings(top))
        save_btn.pack(pady=10, padx=10, fill=tk.X)

    def setup_keybindings_tab(self, parent):
        DarkLabel(parent, text="Configure Keybindings", font=(THEME['font_family_sans'], 12, "bold")).pack(pady=10)
        
        canvas = tk.Canvas(parent, bg=THEME['bg_main'], highlightthickness=0)
        scrollbar = tk.Scrollbar(parent, orient="vertical", command=canvas.yview)
        scrollable_frame = DarkFrame(canvas)
        
        scrollable_frame.bind("<Configure>", lambda e: canvas.configure(scrollregion=canvas.bbox("all")))
        canvas.create_window((0, 0), window=scrollable_frame, anchor="nw")
        canvas.configure(yscrollcommand=scrollbar.set)
        
        canvas.pack(side="left", fill="both", expand=True, padx=10)
        scrollbar.pack(side="right", fill="y")
        
        # Only show keybinding config items (exclude crosshair settings)
        keybinding_keys = ['deselect', 'next_image', 'prev_image', 'cycle_class', 'delete_box', 
                          'copy', 'paste', 'duplicate', 'edit_class', 'undo', 'redo']
        
        for action, key in self.model.config.items():
            # Skip non-keybinding items
            if action not in keybinding_keys:
                continue
            row_frame = DarkFrame(scrollable_frame)
            row_frame.pack(fill=tk.X, pady=2)
            DarkLabel(row_frame, text=action.replace("_", " ").title()).pack(side=tk.LEFT, padx=5)
            btn = DarkButton(row_frame, text=key, width=15)
            btn.pack(side=tk.RIGHT, padx=5)
            btn.configure(command=lambda b=btn, a=action: self.capture_key(b, a))

    def setup_class_management_tab(self, parent):
        DarkLabel(parent, text="Manage Classes", font=(THEME['font_family_sans'], 12, "bold")).pack(pady=10)
        info_text = "Add, remove, or reorder classes. Changes will update all annotation files."
        DarkLabel(parent, text=info_text, wraplength=550).pack(pady=5)
        
        main_frame = DarkFrame(parent)
        main_frame.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        
        list_frame = DarkFrame(main_frame)
        list_frame.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=(0, 10))
        
        list_container = DarkFrame(list_frame)
        list_container.pack(fill=tk.BOTH, expand=True)
        
        self.class_mgmt_listbox = tk.Listbox(list_container, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                                            selectbackground=THEME['selection'], relief=tk.FLAT, bd=0, exportselection=False)
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
        DarkLabel(parent, text="Batch Replace Class IDs", font=(THEME['font_family_sans'], 12, "bold")).pack(pady=10)
        info_text = "Replace all instances of one class ID with another in the directory."
        DarkLabel(parent, text=info_text, wraplength=550).pack(pady=5)
        
        selection_frame = DarkFrame(parent)
        selection_frame.pack(fill=tk.X, padx=10, pady=10)
        
        self.batch_old_listbox = tk.Listbox(selection_frame, height=8, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                                         selectbackground=THEME['selection'], exportselection=False)
        self.batch_old_listbox.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=5)
        
        self.batch_new_listbox = tk.Listbox(selection_frame, height=8, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                                         selectbackground=THEME['selection'], exportselection=False)
        self.batch_new_listbox.pack(side=tk.RIGHT, fill=tk.BOTH, expand=True, padx=5)
        
        self.refresh_batch_listboxes()
            
        DarkButton(parent, text="Execute Batch Replace", command=self.execute_batch_replace, bg=THEME['accent']).pack(pady=10)

    def refresh_batch_listboxes(self):
        if hasattr(self, 'batch_old_listbox') and hasattr(self, 'batch_new_listbox'):
            self.batch_old_listbox.delete(0, tk.END)
            self.batch_new_listbox.delete(0, tk.END)
            for i, cls in enumerate(self.model.classes):
                display_text = f"{cls['id']}: {cls['name']}"
                self.batch_old_listbox.insert(tk.END, display_text)
                self.batch_new_listbox.insert(tk.END, display_text)

    def setup_game_presets_tab(self, parent):
        DarkLabel(parent, text="Switch Game Classes", font=(THEME['font_family_sans'], 12, "bold")).pack(pady=10)
        
        self.preset_listbox = tk.Listbox(parent, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                                         selectbackground=THEME['selection'], exportselection=False)
        self.preset_listbox.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)
        
        self.refresh_preset_list()
        
        btn_frame = DarkFrame(parent)
        btn_frame.pack(fill=tk.X, padx=10, pady=5)
        DarkButton(btn_frame, text="Load Selected", command=self.load_selected_preset, bg=THEME['accent']).pack(side=tk.LEFT, expand=True, fill=tk.X, padx=2)
        DarkButton(btn_frame, text="Refresh", command=self.refresh_preset_list).pack(side=tk.RIGHT, expand=True, fill=tk.X, padx=2)

    def setup_box_presets_tab(self, parent):
        DarkLabel(parent, text="Manage Bounding Box Presets", font=(THEME['font_family_sans'], 12, "bold")).pack(pady=10)
        info_text = "View and manage bounding box slots (1-9).\nPresets are remembered per project and class set across app restarts."
        DarkLabel(parent, text=info_text, wraplength=550).pack(pady=5)
        
        container = DarkFrame(parent)
        container.pack(fill=tk.BOTH, expand=True, padx=10, pady=10)
        
        # Use a listbox to show status of slots
        self.box_presets_lb = tk.Listbox(container, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                                         selectbackground=THEME['selection'], relief=tk.FLAT, bd=0, height=10, exportselection=False)
        self.box_presets_lb.pack(fill=tk.BOTH, expand=True)
        
        self.update_box_presets_list()
        
        btn_frame = DarkFrame(parent)
        btn_frame.pack(fill=tk.X, padx=10, pady=10)
        
        DarkButton(btn_frame, text="Preview Selected Slot", command=self.preview_selected_slot).pack(fill=tk.X, pady=2)
        DarkButton(btn_frame, text="Save Selected/All to Selected Slot", command=self.save_to_selected_slot, bg=THEME['accent']).pack(fill=tk.X, pady=2)
        DarkButton(btn_frame, text="Clear All Presets", command=self.clear_all_presets, bg=THEME['danger']).pack(fill=tk.X, pady=2)

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
        if messagebox.askyesno("Confirm", "Clear all stored box presets for the current project and class set?"):
            self.model.presets.clear()
            self.clear_preview(redraw=False)
            self.model.save_session()
            self.update_box_presets_list()
            self.view.redraw_canvas()
            self.view.update_status("All box presets cleared for this project/class set")

    def save_settings(self, window):
        save_config(CONFIG_FILE, self.model.config)
        self.bind_events()
        window.destroy()
        messagebox.showinfo("Settings", "Settings saved!")
        self.view.redraw_canvas()

    def capture_key(self, button, action):
        cap = tk.Toplevel(self.view.root)
        cap.title("Press Any Key")
        cap.geometry("250x100")
        cap.configure(bg=THEME['bg_main'])
        cap.transient(self.view.root)
        cap.grab_set()
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

        removed = [n for n in old_classes if n not in self.temp_classes]
        confirm_msg = "Apply class changes and update all annotation files?\n\n"
        if removed:
            confirm_msg += ("Removed classes (their annotations will be DELETED):\n"
                            + ", ".join(removed) + "\n\n")
        confirm_msg += "A backup of all annotation files will be created first."
        if not messagebox.askyesno("Confirm", confirm_msg): return

        class_mapping = create_class_mapping(old_classes, self.temp_classes)
        new_classes = list(self.temp_classes)
        output_dir = self.model.output_dir

        if not output_dir:
            self._finish_class_changes(new_classes, None, 0)
            return

        def work(report):
            txt_files = [f for f in os.listdir(output_dir) if f.lower().endswith('.txt')]
            backup_path = None
            if txt_files:
                backup_path = backup_annotations(
                    output_dir,
                    progress_callback=lambda c, t, f: report(c, t, f"Backing up... {c}/{t}"))
                if backup_path is None:
                    raise RuntimeError("Backup failed — class update aborted. No files were changed.")
            updated = 0
            for i, f in enumerate(txt_files):
                if update_annotation_file(os.path.join(output_dir, f), class_mapping, remove_unmapped=True):
                    updated += 1
                report(i + 1, len(txt_files), f"Updating... {i + 1}/{len(txt_files)}")
            return backup_path, updated

        def done(result):
            if isinstance(result, Exception):
                messagebox.showerror("Error", str(result))
                return
            backup_path, updated = result
            self._finish_class_changes(new_classes, backup_path, updated)

        self.run_threaded_task("Updating Classes", work, done)

    def _finish_class_changes(self, new_classes, backup_path, updated_count):
        save_classes(CLASSES_FILE, new_classes)
        self.model.classes = load_classes(CLASSES_FILE)
        self.refresh_class_dependent_views()
        self.refresh_preset_scope()
        if self.model.current_image_index != -1:
            # Suppress the autosave in load_image: the in-memory boxes still
            # carry pre-remap class ids and would clobber the updated file.
            index = self.model.current_image_index
            self.model.current_image_index = -1
            self.load_image(index)
        self.refresh_class_counts()
        msg = f"Classes updated! {updated_count} annotation file(s) rewritten."
        if backup_path:
            msg += f"\nBackup: {os.path.basename(backup_path)}"
        messagebox.showinfo("Success", msg)

    def execute_batch_replace(self):
        old_sel = self.batch_old_listbox.curselection()
        new_sel = self.batch_new_listbox.curselection()
        if not old_sel or not new_sel: 
            messagebox.showwarning("Warning", "Please select both old and new classes.")
            return
        
        old_id = self.model.classes[old_sel[0]]['id']
        new_id = self.model.classes[new_sel[0]]['id']
        
        old_name = self.model.classes[old_sel[0]]['name']
        new_name = self.model.classes[new_sel[0]]['name']
        
        if old_id == new_id:
            messagebox.showwarning("Warning", "Old and new class IDs are the same.")
            return
            
        if not self.model.output_dir:
            messagebox.showerror("Error", "No output directory (labels folder) loaded.")
            return

        txt_files = [f for f in os.listdir(self.model.output_dir) if f.lower().endswith('.txt')]
        if not txt_files:
            messagebox.showinfo("Info", "No annotation files found in the output directory.")
            return
            
        confirm_msg = f"Replace all ID {old_id} ({old_name}) with {new_id} ({new_name})?\n\n"
        confirm_msg += f"Directory: {self.model.output_dir}\n"
        confirm_msg += f"Files to process: {len(txt_files)}\n"
        confirm_msg += "A backup will be created before making changes."
        
        if not messagebox.askyesno("Confirm Batch Replace", confirm_msg):
            return

        mapping = {old_id: new_id}
        output_dir = self.model.output_dir

        def work(report):
            backup_path = backup_annotations(
                output_dir,
                progress_callback=lambda c, t, f: report(c, t, f"Backing up... {c}/{t}"))
            if backup_path is None:
                raise RuntimeError("Backup failed — batch replace aborted. No files were changed.")
            updated = 0
            for i, f in enumerate(txt_files):
                if update_annotation_file(os.path.join(output_dir, f), mapping):
                    updated += 1
                report(i + 1, len(txt_files), f"Updating... {i + 1}/{len(txt_files)}")
            return backup_path, updated

        def done(result):
            if isinstance(result, Exception):
                messagebox.showerror("Error", str(result))
                return
            backup_path, updated_count = result

            # Update in-memory boxes for the current image so auto-save doesn't overwrite our changes
            for box in self.model.boxes:
                if box['class_id'] == old_id:
                    box['class_id'] = new_id

            if self.model.current_image_index != -1:
                self.view.redraw_canvas()
                self.view.update_box_list()

            self.refresh_class_counts()
            messagebox.showinfo(
                "Success",
                f"Batch replace complete! Updated {updated_count} files.\n"
                f"Backup: {os.path.basename(backup_path)}")

        self.run_threaded_task("Batch Replacing", work, done)

    def refresh_preset_list(self):
        self.preset_listbox.delete(0, tk.END)
        if os.path.exists(DATA_DIR):
            presets = [f for f in os.listdir(DATA_DIR) if f.endswith('.txt') and f != 'predefined_classes.txt']
            for p in sorted(presets):
                self.preset_listbox.insert(tk.END, p)

    def load_selected_preset(self):
        sel = self.preset_listbox.curselection()
        if not sel: return

        preset = self.preset_listbox.get(sel[0])
        if messagebox.askyesno("Confirm", f"Overwrite current classes with {preset}?"):
            shutil.copy2(os.path.join(DATA_DIR, preset), CLASSES_FILE)
            self.model.classes = load_classes(CLASSES_FILE)
            self.refresh_class_dependent_views()
            self.refresh_preset_scope()
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
            self.model.save_state()
            self.model.resize_mode = True
            self.model.active_handle_index = handle_name
            self.model.active_box_index = box_idx
            self.model.start_x, self.model.start_y = cx, cy
            return

        # 2. Check for box selection
        clicked_box_idx = self.find_box_at(x, y)
        if clicked_box_idx != -1:
            if event.state & 0x1: # Shift key
                if clicked_box_idx in self.model.selected_indices:
                    self.model.selected_indices.remove(clicked_box_idx)
                else:
                    self.model.selected_indices.add(clicked_box_idx)
            else:
                # If clicking on an already-selected box, keep the multi-selection
                # Only replace selection if clicking on an unselected box
                if clicked_box_idx not in self.model.selected_indices:
                    self.model.selected_indices = {clicked_box_idx}
            
            self.model.save_state()
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
                    except (ValueError, OSError): pass
                return None

            with concurrent.futures.ThreadPoolExecutor() as executor:
                results = list(executor.map(check_file, self.model.full_image_list))
            
            filtered = [r for r in results if r is not None]
            self._ui_call(lambda: self.finish_filter(filtered))

        threading.Thread(target=scan_thread, daemon=True).start()

    def finish_filter(self, filtered_images):
        self.view.root.config(cursor="")
        self._save_current_before_list_change()
        self.model.image_list = filtered_images
        self._refresh_file_list()

        if self.model.image_list:
            self.load_image(0)
        else:
            messagebox.showinfo("Filter", "No images found for this class.")

    def clear_image_filter(self):
        self._save_current_before_list_change()
        self.model.image_list = list(self.model.full_image_list)
        self._refresh_file_list()
        if self.model.image_list:
            self.load_image(0)
        self.view.filter_combo.set("")

    def on_canvas_drag(self, event):
        if not self.model.current_image_pil: return
        x = self.view.canvas.canvasx(event.x)
        y = self.view.canvas.canvasy(event.y)
        iw, ih = self.model.current_image_pil.size
        
        # Update crosshair during drag
        self.view.draw_crosshair(x, y)
        
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
                                             outline=THEME['selected_box'], dash=(4, 4), tags="temp_rect")
            
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
            self.view.move_box_items(self.model.active_box_index)

        elif self.model.move_mode:
            dx = (cx - self.model.start_x) / self.view.scale
            dy = (cy - self.model.start_y) / self.view.scale

            selected_boxes = []
            for idx in self.model.selected_indices:
                box = self.model.boxes[idx]
                x1, y1, x2, y2 = denormalize_box(box, iw, ih)
                selected_boxes.append((idx, x1, y1, x2, y2, box['class_id']))

            if not selected_boxes:
                return

            group_min_x = min(x1 for _, x1, _, _, _, _ in selected_boxes)
            group_min_y = min(y1 for _, _, y1, _, _, _ in selected_boxes)
            group_max_x = max(x2 for _, _, _, x2, _, _ in selected_boxes)
            group_max_y = max(y2 for _, _, _, _, y2, _ in selected_boxes)

            dx = max(-group_min_x, min(dx, iw - group_max_x))
            dy = max(-group_min_y, min(dy, ih - group_max_y))

            for idx, x1, y1, x2, y2, class_id in selected_boxes:
                new_box = normalize_box(x1 + dx, y1 + dy, x2 + dx, y2 + dy, iw, ih)
                new_box['class_id'] = class_id
                self.model.boxes[idx] = new_box

            self.model.start_x += dx * self.view.scale
            self.model.start_y += dy * self.view.scale
            for idx in self.model.selected_indices:
                self.view.move_box_items(idx)

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
        self.view.update_box_list()

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
            self.view.update_box_list()
            self.view.update_status("Action undone")

    def redo(self):
        if self.model.redo():
            self.view.redraw_canvas()
            self.view.update_box_list()
            self.view.update_status("Action redone")

    def on_canvas_configure(self, event):
        """Cheap live redraw while resizing; sharpen once the resize settles."""
        self.view.redraw_canvas(fast=True)
        if self.redraw_timer:
            self.view.root.after_cancel(self.redraw_timer)
        self.redraw_timer = self.view.root.after(200, lambda: self.view.redraw_canvas(fast=False))

    def start_pan(self, event):
        self.view.canvas.scan_mark(event.x, event.y)

    def on_pan(self, event):
        self.view.canvas.scan_dragto(event.x, event.y, gain=1)

    def on_zoom(self, event):
        # Coalesce wheel ticks: accumulate the multiplier and redraw at most
        # once per ~15ms instead of once per event (redraws are the expensive
        # part; rapid ticks collapse losslessly into one multiplicative step).
        self._pending_zoom *= 1.1 if event.delta > 0 else 1 / 1.1
        self._zoom_anchor = (event.x, event.y)
        if self._zoom_job is None:
            self._zoom_job = self.view.root.after(15, self._apply_pending_zoom)

    def _apply_pending_zoom(self):
        self._zoom_job = None
        multiplier, anchor = self._pending_zoom, self._zoom_anchor
        self._pending_zoom, self._zoom_anchor = 1.0, None
        if anchor and multiplier != 1.0:
            self._zoom_at(anchor[0], anchor[1], multiplier)

    def zoom_in(self):
        self._zoom_at(self.view.canvas.winfo_width() / 2,
                      self.view.canvas.winfo_height() / 2, 1.1)

    def zoom_out(self):
        self._zoom_at(self.view.canvas.winfo_width() / 2,
                      self.view.canvas.winfo_height() / 2, 1 / 1.1)

    def zoom_reset(self):
        """Reset zoom so the image fits the window."""
        if not self.model.current_image_pil: return
        self.model.zoom_factor = 1.0
        self.view.canvas.xview_moveto(0)
        self.view.canvas.yview_moveto(0)
        self.view.redraw_canvas()

    def _zoom_at(self, wx, wy, multiplier):
        """Zoom keeping the image point under window coords (wx, wy) fixed."""
        if not self.model.current_image_pil: return

        # 1. Window coords -> canvas-space -> image-space
        mx = self.view.canvas.canvasx(wx)
        my = self.view.canvas.canvasy(wy)
        if self.view.scale == 0: return
        ix = (mx - self.view.offset_x) / self.view.scale
        iy = (my - self.view.offset_y) / self.view.scale

        # 2. Update zoom factor (clamped)
        self.model.zoom_factor = max(0.1, min(10.0, self.model.zoom_factor * multiplier))

        # 3. Redraw (fast mode) to calculate new scale and dimensions
        self.view.redraw_canvas(fast=True)

        # 4. Re-anchor the scroll so the image point stays under (wx, wy)
        nx = ix * self.view.scale + self.view.offset_x
        ny = iy * self.view.scale + self.view.offset_y
        target_vx = nx - wx
        target_vy = ny - wy

        scrollregion = self.view.canvas.cget('scrollregion').split()
        if len(scrollregion) == 4:
            sr_x1, sr_y1, sr_x2, sr_y2 = scrollregion
            sr_w = float(sr_x2) - float(sr_x1)
            sr_h = float(sr_y2) - float(sr_y1)
            if sr_w > 0: self.view.canvas.xview_moveto(target_vx / sr_w)
            if sr_h > 0: self.view.canvas.yview_moveto(target_vy / sr_h)

        # 5. Debounce high-quality redraw
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
        # Find all items with "handle" tag and check if click is within their bounds
        handle_items = self.view.canvas.find_withtag("handle")
        h_size = 8  # Slightly larger detection area than the drawn handle (6)
        
        for item in handle_items:
            # Get the bounding box of this handle
            bbox = self.view.canvas.bbox(item)
            if bbox:
                x1, y1, x2, y2 = bbox
                # Expand the detection area slightly for easier clicking
                if (x1 - h_size <= x <= x2 + h_size) and (y1 - h_size <= y <= y2 + h_size):
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
            self.view.update_box_list()

    def copy_boxes(self):
        if self._is_input_focused(): return
        if self.model.selected_indices:
            self.model.clipboard = [self.model.boxes[i].copy() for i in self.model.selected_indices]
        else:
            self.model.clipboard = [b.copy() for b in self.model.boxes]
        if self.model.preview_visible and self.model.preview_source == 'clipboard':
            self.model.set_preview(self.model.clipboard, 'clipboard')
            self.view.redraw_canvas()
        messagebox.showinfo("Info", f"Copied {len(self.model.clipboard)} boxes.")

    def paste_boxes(self):
        if self._is_input_focused(): return
        if not self.model.clipboard: return
        
        self.model.save_state()
        for box in self.model.clipboard:
            self.model.boxes.append(box.copy())
        self.clear_preview(redraw=False)
        self.view.redraw_canvas()
        self.view.update_box_list()
        self.view.update_status(f"Pasted {len(self.model.clipboard)} box(es)")

    def duplicate_boxes(self):
        """Duplicate selected boxes (or all if none selected) with a small offset."""
        if self._is_input_focused(): return
        
        # Determine which boxes to duplicate
        if self.model.selected_indices:
            boxes_to_dup = [self.model.boxes[i].copy() for i in self.model.selected_indices]
        else:
            boxes_to_dup = [b.copy() for b in self.model.boxes]
        
        if not boxes_to_dup:
            return
        
        self.model.save_state()
        
        # Offset for duplicated boxes (small shift so they're visible)
        offset = 0.02  # 2% of image dimension
        
        new_indices = []
        for box in boxes_to_dup:
            new_box = box.copy()
            # Shift the duplicate slightly down and right (YOLO format uses x_center, y_center)
            new_box['x_center'] = min(1.0 - new_box['w'] / 2, new_box['x_center'] + offset)
            new_box['y_center'] = min(1.0 - new_box['h'] / 2, new_box['y_center'] + offset)
            self.model.boxes.append(new_box)
            new_indices.append(len(self.model.boxes) - 1)
        
        # Select the newly duplicated boxes
        self.model.selected_indices = set(new_indices)
        
        self.view.redraw_canvas()
        self.view.update_box_list()
        self.view.update_status(f"Duplicated {len(boxes_to_dup)} box(es)")

    def save_preset(self, slot):
        if self._is_input_focused():
            return

        if self.model.selected_indices:
            boxes_to_save = [self.model.boxes[i].copy() for i in self.model.selected_indices]
            source = "selected"
        else:
            boxes_to_save = [b.copy() for b in self.model.boxes]
            source = "all"
        
        if not boxes_to_save:
            self.view.update_status(f"Preset {slot} remains empty (no boxes to save)")
            return

        self.model.presets[slot] = boxes_to_save
        self.model.save_session()
        if self.model.preview_visible and self.model.preview_source == 'preset' and self.model.preview_slot == slot:
            self.model.set_preview(boxes_to_save, 'preset', slot=slot)
            self.view.redraw_canvas()
        self.view.update_status(f"Stored {len(boxes_to_save)} {source} boxes in Preset {slot}")

    def apply_preset(self, slot):
        if self._is_input_focused():
            return

        preset_boxes = self.model.presets.get(slot)
        if not preset_boxes:
            self.view.update_status(f"Preset {slot} is empty. Use Ctrl+Shift+{slot} to save current boxes.")
            return
        
        self.model.save_state()
        for box in preset_boxes:
            self.model.boxes.append(box.copy())
        self.clear_preview(redraw=False)
        self.view.redraw_canvas()
        self.view.update_box_list()
        self.view.update_status(f"Applied Preset {slot} ({len(preset_boxes)} boxes)")

    def setup_themes_tab(self, parent):
        """Setup the themes configuration tab."""
        from src.themes import PREDEFINED_THEMES
        
        DarkLabel(parent, text="UI Themes", font=(THEME['font_family_sans'], 12, "bold")).pack(pady=10)
        
        lb_frame = DarkFrame(parent)
        lb_frame.pack(fill=tk.BOTH, expand=True, padx=10, pady=5)
        
        theme_lb = tk.Listbox(lb_frame, bg=THEME['list_bg'], fg=THEME['fg_text'], 
                               selectbackground=THEME['selection'], relief=tk.FLAT, bd=0, exportselection=False)
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
