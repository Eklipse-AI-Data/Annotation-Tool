import tkinter as tk
from tkinter import ttk

from src.themes import DEFAULT_THEME

# Dynamic Theme Reference
THEME = DEFAULT_THEME.copy()

def update_ui_theme(new_theme_dict):
    """Update the global THEME dictionary with new values."""
    THEME.update(new_theme_dict)

class ScrollableFrame(tk.Frame):
    """A frame that can be scrolled vertically."""
    def __init__(self, master, **kwargs):
        self.bg = kwargs.get('bg', THEME['bg_main'])
        super().__init__(master, **kwargs)
        
        # Create a canvas and a scrollbar
        self.canvas = tk.Canvas(self, bg=self.bg, highlightthickness=0)
        self.scrollbar = tk.Scrollbar(self, orient="vertical", command=self.canvas.yview)
        
        # Create a frame inside the canvas to hold actual content
        self.scrollable_content = tk.Frame(self.canvas, bg=self.bg)
        
        # Bind the frame to the canvas
        self.scrollable_content.bind(
            "<Configure>",
            lambda e: self.canvas.configure(scrollregion=self.canvas.bbox("all"))
        )
        
        self.canvas_window = self.canvas.create_window((0, 0), window=self.scrollable_content, anchor="nw")
        
        # Bind canvas resize to update content width
        self.canvas.bind("<Configure>", self._on_canvas_configure)
        
        self.canvas.configure(yscrollcommand=self.scrollbar.set)
        
        # Pack everything
        self.canvas.pack(side="left", fill="both", expand=True)
        self.scrollbar.pack(side="right", fill="y")
        
        # Mouse wheel scrolling
        self.canvas.bind_all("<MouseWheel>", self._on_mousewheel)

    def _on_canvas_configure(self, event):
        """Update the width of the scrollable frame to match the canvas."""
        self.canvas.itemconfigure(self.canvas_window, width=event.width)

    def _on_mousewheel(self, event):
        """Handle mouse wheel scrolling."""
        # Only scroll if the widget or its child is under the mouse
        if self.body_is_visible():
            self.canvas.yview_scroll(int(-1*(event.delta/120)), "units")

    def body_is_visible(self):
        """Check if this frame is visible/mapped."""
        try:
            return self.winfo_ismapped()
        except:
            return False

class TabbedFrame(tk.Frame):
    """A frame with tabs for switching between different content areas."""
    def __init__(self, master, **kwargs):
        self.bg = kwargs.get('bg', THEME['bg_sidebar'])
        super().__init__(master, **kwargs)
        self.configure(bg=self.bg)
        
        self.tabs = {} # name: (button, frame)
        self.active_tab = None
        
        # Header for tab buttons
        self.header = tk.Frame(self, bg=self.bg)
        self.header.pack(fill=tk.X, side=tk.TOP)
        
        # Content area
        self.content = tk.Frame(self, bg=self.bg)
        self.content.pack(fill=tk.BOTH, expand=True)

    def add_tab(self, name):
        """Create a new tab and return its frame."""
        # Tab Button
        btn = tk.Button(self.header, text=name, bg=THEME['button_bg'], fg=THEME['button_fg'],
                        activebackground=THEME['button_highlight'], relief=tk.FLAT, bd=0, 
                        padx=10, pady=5, font=(THEME['font_family_sans'], THEME['font_size_main']-1, 'bold'), cursor='hand2',
                        command=lambda n=name: self.show_tab(n))
        btn.pack(side=tk.LEFT, fill=tk.X, expand=True)
        
        # Content frame
        tab_frame = tk.Frame(self.content, bg=self.bg)
        
        self.tabs[name] = (btn, tab_frame)
        
        if not self.active_tab:
            self.show_tab(name)
            
        return tab_frame

    def show_tab(self, name):
        """Switch to the specified tab."""
        if name not in self.tabs: return
        
        # Hide current
        if self.active_tab:
            self.tabs[self.active_tab][1].pack_forget()
            self.tabs[self.active_tab][0].configure(bg=THEME['button_bg'], fg=THEME['button_fg'])
        
        # Show new
        self.active_tab = name
        self.tabs[name][1].pack(fill=tk.BOTH, expand=True)
        self.tabs[name][0].configure(bg=THEME['button_highlight'], fg=THEME['fg_highlight'])

class CollapsibleFrame(tk.Frame):
    """An expandable/collapsible frame with a header."""
    def __init__(self, master, text, expanded=True, **kwargs):
        self.bg = kwargs.get('bg', THEME['bg_sidebar'])
        super().__init__(master, **kwargs)
        self.configure(bg=self.bg)
        
        self.is_expanded = expanded
        
        # Toggle Button Header
        self.header = tk.Button(self, text=f"▼ {text}", bg=self.bg, fg=THEME['button_highlight'],
                                activebackground=self.bg, activeforeground=THEME['fg_highlight'],
                                relief=tk.FLAT, bd=0, anchor='w', font=(THEME['font_family_serif'], THEME['font_size_header'], 'bold'),
                                cursor='hand2', command=self.toggle)
        self.header.pack(fill=tk.X)
        self.text = text
        
        # Content area
        self.content = tk.Frame(self, bg=self.bg)
        if self.is_expanded:
            self.content.pack(fill=tk.BOTH, expand=True)
        else:
            self.header.configure(text=f"▶ {text}")

    def toggle(self):
        if self.is_expanded:
            self.content.pack_forget()
            self.header.configure(text=f"▶ {self.text}")
        else:
            self.content.pack(fill=tk.BOTH, expand=True)
            self.header.configure(text=f"▼ {self.text}")
        self.is_expanded = not self.is_expanded

class DarkFrame(tk.Frame):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_main'])
        super().__init__(master, **kwargs)

class SidebarFrame(tk.Frame):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_sidebar'])
        super().__init__(master, **kwargs)

class DarkLabel(tk.Label):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_main'])
        kwargs.setdefault('fg', THEME['fg_text'])
        kwargs.setdefault('font', (THEME['font_family_sans'], THEME['font_size_main']))
        super().__init__(master, **kwargs)

class SectionLabel(tk.Label):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_sidebar'])
        kwargs.setdefault('fg', THEME['button_highlight'])  # Neon cyan for headers
        kwargs.setdefault('font', (THEME['font_family_serif'], THEME['font_size_header'], 'bold'))
        kwargs.setdefault('pady', 5)
        super().__init__(master, **kwargs)

class DarkButton(tk.Button):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['button_bg'])
        kwargs.setdefault('fg', THEME['button_fg'])
        kwargs.setdefault('activebackground', THEME['button_highlight'])
        kwargs.setdefault('activeforeground', THEME['fg_highlight'])
        kwargs.setdefault('relief', tk.FLAT)
        kwargs.setdefault('bd', 0)
        kwargs.setdefault('padx', 20)
        kwargs.setdefault('pady', 8)
        kwargs.setdefault('font', (THEME['font_family_sans'], THEME['font_size_main']))
        kwargs.setdefault('cursor', 'hand2')
        super().__init__(master, **kwargs)
        
        # Add hover effect
        self.bind('<Enter>', self.on_enter)
        self.bind('<Leave>', self.on_leave)

    def on_enter(self, e):
        if self['state'] != 'disabled':
            self['bg'] = THEME['button_hover']

    def on_leave(self, e):
        if self['state'] != 'disabled':
            self['bg'] = THEME['button_bg']

class DarkEntry(tk.Entry):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['entry_bg'])
        kwargs.setdefault('fg', THEME['entry_fg'])
        kwargs.setdefault('insertbackground', THEME['fg_text'])
        kwargs.setdefault('relief', tk.FLAT)
        kwargs.setdefault('bd', 0)
        kwargs.setdefault('font', (THEME['font_family_sans'], THEME['font_size_main']))
        super().__init__(master, **kwargs)
        
        # Add padding effect with a wrapper if needed
        self.configure(highlightthickness=1, highlightbackground=THEME['border'], highlightcolor=THEME['accent'])

class DarkListbox(tk.Listbox):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['list_bg'])
        kwargs.setdefault('fg', THEME['entry_fg'])
        kwargs.setdefault('selectbackground', THEME['selection'])
        kwargs.setdefault('selectforeground', THEME['fg_highlight'])
        kwargs.setdefault('relief', tk.FLAT)
        kwargs.setdefault('bd', 0)
        kwargs.setdefault('highlightthickness', 0)
        kwargs.setdefault('font', (THEME['font_family_sans'], THEME['font_size_main']))
        kwargs.setdefault('activestyle', 'none')
        super().__init__(master, **kwargs)

class DarkScrollbar(ttk.Scrollbar):
    def __init__(self, master, **kwargs):
        super().__init__(master, **kwargs)

class DarkProgressBar(tk.Canvas):
    """Custom progress bar matching the dark theme."""
    def __init__(self, master, **kwargs):
        self.height = kwargs.pop('height', 20)
        self.bg_color = kwargs.pop('bg', THEME['entry_bg'])
        self.fg_color = kwargs.pop('fg', THEME['button_highlight'])
        self.text_color = kwargs.pop('text_color', THEME['fg_text'])
        
        super().__init__(master, height=self.height, bg=self.bg_color, 
                         highlightthickness=1, highlightbackground=THEME['border'], **kwargs)
        
        self._value = 0
        self._max_value = 100
        self._text = ""
        
        self.bind('<Configure>', self._on_resize)
    
    def set_value(self, value, max_value=None, text=None):
        """Update progress bar value and optionally max value and text."""
        self._value = value
        if max_value is not None:
            self._max_value = max_value
        if text is not None:
            self._text = text
        self._draw()
    
    def _draw(self):
        """Redraw the progress bar."""
        self.delete("all")
        
        width = self.winfo_width()
        height = self.winfo_height()
        
        if width <= 1:
            return
        
        # Calculate fill width
        if self._max_value > 0:
            fill_width = (self._value / self._max_value) * width
        else:
            fill_width = 0
        
        # Draw progress fill
        if fill_width > 0:
            self.create_rectangle(0, 0, fill_width, height, 
                                  fill=self.fg_color, outline="")
        
        # Draw text centered
        if self._text:
            self.create_text(width / 2, height / 2, text=self._text,
                           fill=self.text_color, font=(THEME['font_family_sans'], THEME['font_size_main']-1, 'bold'))
    
    def _on_resize(self, event):
        """Handle resize events."""
        self._draw()

