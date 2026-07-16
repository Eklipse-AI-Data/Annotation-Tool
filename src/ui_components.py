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
        except tk.TclError:
            return False

    def apply_theme(self):
        self.bg = THEME['bg_sidebar']
        self.configure(bg=self.bg)
        self.canvas.configure(bg=self.bg)
        self.scrollable_content.configure(bg=self.bg)

class TabbedFrame(tk.Frame):
    """A frame with tabs for switching between different content areas."""
    def __init__(self, master, **kwargs):
        self.bg = kwargs.get('bg', THEME['bg_sidebar'])
        super().__init__(master, **kwargs)
        self.configure(bg=self.bg)
        
        self.tabs = {} # name: (button, frame)
        self.active_tab = None
        
        # Header for tab buttons - using grid for equal width
        self.header = tk.Frame(self, bg=self.bg)
        self.header.pack(fill=tk.X, side=tk.TOP, padx=5, pady=(5, 0))
        
        self._tab_count = 0
        
        # Content area
        self.content = tk.Frame(self, bg=self.bg)
        self.content.pack(fill=tk.BOTH, expand=True)

    def add_tab(self, name):
        """Create a new tab and return its frame."""
        # Configure grid column for equal weight
        self.header.grid_columnconfigure(self._tab_count, weight=1, uniform="tabs")
        
        # Modern Tab Button using Frame with Label for better sizing
        btn = TabButton(self.header, text=name, command=lambda n=name: self.show_tab(n))
        btn.grid(row=0, column=self._tab_count, sticky="nsew", padx=2)
        
        self._tab_count += 1
        
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
            self.tabs[self.active_tab][0].set_active(False)
        
        # Show new
        self.active_tab = name
        self.tabs[name][1].pack(fill=tk.BOTH, expand=True)
        self.tabs[name][0].set_active(True)

    def apply_theme(self):
        self.bg = THEME['bg_sidebar']
        self.configure(bg=self.bg)
        self.header.configure(bg=self.bg)
        self.content.configure(bg=self.bg)
        for btn, frame in self.tabs.values():
            btn.apply_theme()
            frame.configure(bg=self.bg)


class TabButton(tk.Canvas):
    """Modern rounded tab button."""
    def __init__(self, master, text, command=None, **kwargs):
        self.text = text
        self.command = command
        self.corner_radius = 8
        self.font = (THEME['font_family_sans'], THEME['font_size_main']-1, 'bold')
        
        self.bg_color = THEME['button_bg']
        self.fg_color = THEME['button_fg']
        self.active_bg = THEME['button_highlight']
        self.active_fg = THEME['fg_highlight']
        self.hover_color = THEME['button_hover']
        
        self._active = False
        self._hovering = False
        
        bg = master.cget('bg') if hasattr(master, 'cget') else THEME['bg_sidebar']
        super().__init__(master, highlightthickness=0, bg=bg, height=32, **kwargs)
        
        self.configure(cursor='hand2')
        
        self.bind('<Enter>', self._on_enter)
        self.bind('<Leave>', self._on_leave)
        self.bind('<ButtonRelease-1>', self._on_click)
        self.bind('<Configure>', lambda e: self._draw())
        
        self.after(10, self._draw)
    
    def _draw(self):
        self.delete('all')
        w = self.winfo_width()
        h = self.winfo_height()
        if w <= 1: return
        
        r = self.corner_radius
        
        # Determine colors
        if self._active:
            fill = self.active_bg
            text_fill = self.active_fg
        elif self._hovering:
            fill = self.hover_color
            text_fill = self.fg_color
        else:
            fill = self.bg_color
            text_fill = self.fg_color
        
        # Draw tab shape with rounded TOP corners only, flat bottom
        # Using a polygon with explicit points for rounded top corners
        points = []
        
        # Bottom left corner (sharp)
        points.extend([0, h])
        
        # Left side up to top-left curve
        points.extend([0, r])
        
        # Top-left rounded corner (approximated with several points)
        import math
        for i in range(5):
            angle = math.pi - (math.pi / 2) * (i / 4)
            px = r + r * math.cos(angle)
            py = r - r * math.sin(angle)
            points.extend([px, py])
        
        # Top edge
        points.extend([r, 0])
        points.extend([w - r, 0])
        
        # Top-right rounded corner
        for i in range(5):
            angle = math.pi / 2 - (math.pi / 2) * (i / 4)
            px = (w - r) + r * math.cos(angle)
            py = r - r * math.sin(angle)
            points.extend([px, py])
        
        # Right side down
        points.extend([w, r])
        points.extend([w, h])
        
        # Bottom edge (sharp corners)
        points.extend([0, h])
        
        self.create_polygon(points, fill=fill, outline='', smooth=False)
        
        # Text
        self.create_text(w // 2, h // 2, text=self.text, fill=text_fill, font=self.font)
    
    def _on_enter(self, e):
        self._hovering = True
        self._draw()
    
    def _on_leave(self, e):
        self._hovering = False
        self._draw()
    
    def _on_click(self, e):
        if self.command:
            self.command()
    
    def set_active(self, active):
        self._active = active
        self._draw()

    def apply_theme(self):
        self.font = (THEME['font_family_sans'], THEME['font_size_main']-1, 'bold')
        self.bg_color = THEME['button_bg']
        self.fg_color = THEME['button_fg']
        self.active_bg = THEME['button_highlight']
        self.active_fg = THEME['fg_highlight']
        self.hover_color = THEME['button_hover']
        tk.Canvas.configure(self, bg=self.master.cget('bg'))
        self._draw()

    def configure(self, **kwargs):
        if 'bg' in kwargs:
            self.bg_color = kwargs.pop('bg')
        if 'fg' in kwargs:
            self.fg_color = kwargs.pop('fg')
        if 'activebackground' in kwargs:
            self.active_bg = kwargs.pop('activebackground')
        super().configure(**kwargs)
        self._draw()

    config = configure

class CollapsibleHeader(tk.Canvas):
    """Modern collapsible section header."""
    def __init__(self, master, text, expanded=True, command=None, **kwargs):
        self.text = text
        self.command = command
        self.expanded = expanded
        self.font = (THEME['font_family_serif'], THEME['font_size_header'], 'bold')
        
        self.bg_color = THEME['bg_sidebar']
        self.fg_color = THEME['button_highlight']
        
        bg = master.cget('bg') if hasattr(master, 'cget') else THEME['bg_sidebar']
        super().__init__(master, highlightthickness=0, bg=bg, height=30, **kwargs)
        
        self.configure(cursor='hand2')
        
        self.bind('<ButtonRelease-1>', self._on_click)
        self.bind('<Configure>', lambda e: self._draw())
        
        self.after(10, self._draw)
    
    def _draw(self):
        self.delete('all')
        w = self.winfo_width()
        h = self.winfo_height()
        if w <= 1: return
        
        # Arrow indicator
        arrow = "▼" if self.expanded else "▶"
        display_text = f"{arrow}  {self.text}"
        
        self.create_text(10, h // 2, text=display_text, fill=self.fg_color, font=self.font, anchor='w')
    
    def _on_click(self, e):
        if self.command:
            self.command()
    
    def set_expanded(self, expanded):
        self.expanded = expanded
        self._draw()

    def apply_theme(self):
        self.font = (THEME['font_family_serif'], THEME['font_size_header'], 'bold')
        self.bg_color = THEME['bg_sidebar']
        self.fg_color = THEME['button_highlight']
        tk.Canvas.configure(self, bg=self.master.cget('bg'))
        self._draw()

    def configure(self, **kwargs):
        if 'text' in kwargs:
            # Parse out the arrow if present
            text = kwargs.pop('text')
            if text.startswith('▼ '):
                self.text = text[2:]
                self.expanded = True
            elif text.startswith('▶ '):
                self.text = text[2:]
                self.expanded = False
            else:
                self.text = text
        if 'bg' in kwargs:
            self.bg_color = kwargs.pop('bg')
        if 'fg' in kwargs:
            self.fg_color = kwargs.pop('fg')
        if 'activebackground' in kwargs:
            kwargs.pop('activebackground')  # Ignore, we handle hover differently
        if 'activeforeground' in kwargs:
            kwargs.pop('activeforeground')
        if 'font' in kwargs:
            self.font = kwargs.pop('font')
        super().configure(**kwargs)
        self._draw()
    
    config = configure


class CollapsibleFrame(tk.Frame):
    """An expandable/collapsible frame with a header."""
    def __init__(self, master, text, expanded=True, **kwargs):
        self.bg = kwargs.get('bg', THEME['bg_sidebar'])
        super().__init__(master, **kwargs)
        self.configure(bg=self.bg)
        
        self.is_expanded = expanded
        self.text = text
        
        # Modern Toggle Header
        self.header = CollapsibleHeader(self, text=text, expanded=expanded, command=self.toggle)
        self.header.pack(fill=tk.X, padx=2, pady=2)
        
        # Content area
        self.content = tk.Frame(self, bg=self.bg)
        if self.is_expanded:
            self.content.pack(fill=tk.BOTH, expand=True)

    def toggle(self):
        if self.is_expanded:
            self.content.pack_forget()
            self.header.set_expanded(False)
        else:
            self.content.pack(fill=tk.BOTH, expand=True)
            self.header.set_expanded(True)
        self.is_expanded = not self.is_expanded

    def apply_theme(self):
        self.bg = THEME['bg_sidebar']
        self.configure(bg=self.bg)
        self.header.apply_theme()
        self.content.configure(bg=self.bg)

class DarkFrame(tk.Frame):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_main'])
        # Remember which theme surface this frame belongs to so theme
        # switches can retint it without guessing from the widget tree.
        self._theme_bg_key = 'bg_sidebar' if kwargs['bg'] == THEME['bg_sidebar'] else 'bg_main'
        super().__init__(master, **kwargs)

    def apply_theme(self):
        self.configure(bg=THEME[self._theme_bg_key])

class SidebarFrame(tk.Frame):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_sidebar'])
        super().__init__(master, **kwargs)

    def apply_theme(self):
        self.configure(bg=THEME['bg_sidebar'])

class DarkLabel(tk.Label):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_main'])
        kwargs.setdefault('fg', THEME['fg_text'])
        kwargs.setdefault('font', (THEME['font_family_sans'], THEME['font_size_main']))
        self._theme_bg_key = 'bg_sidebar' if kwargs['bg'] == THEME['bg_sidebar'] else 'bg_main'
        super().__init__(master, **kwargs)

    def apply_theme(self):
        self.configure(bg=THEME[self._theme_bg_key], fg=THEME['fg_text'],
                       font=(THEME['font_family_sans'], THEME['font_size_main']))

class SectionLabel(tk.Label):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_sidebar'])
        kwargs.setdefault('fg', THEME['button_highlight'])  # Neon cyan for headers
        kwargs.setdefault('font', (THEME['font_family_serif'], THEME['font_size_header'], 'bold'))
        kwargs.setdefault('pady', 5)
        super().__init__(master, **kwargs)

    def apply_theme(self):
        self.configure(bg=THEME['bg_sidebar'], fg=THEME['button_highlight'],
                       font=(THEME['font_family_serif'], THEME['font_size_header'], 'bold'))

class DarkButton(tk.Canvas):
    """Modern rounded button with smooth hover effects."""
    def __init__(self, master, **kwargs):
        # Extract button-specific options
        self.text = kwargs.pop('text', '')
        self.command = kwargs.pop('command', None)
        self.width = kwargs.pop('width', None)
        self.padx = kwargs.pop('padx', 20)
        self.pady = kwargs.pop('pady', 8)
        self.font = kwargs.pop('font', (THEME['font_family_sans'], THEME['font_size_main']))
        self.corner_radius = kwargs.pop('corner_radius', 10)
        
        # Colors
        self.bg_color = kwargs.pop('bg', THEME['button_bg'])
        self.fg_color = kwargs.pop('fg', THEME['button_fg'])
        self.hover_color = kwargs.pop('activebackground', THEME['button_hover'])
        self.active_color = kwargs.pop('activeforeground', THEME['button_highlight'])
        
        # State
        self._state = 'normal'
        self._pressed = False
        self._current_bg = self.bg_color
        self._width = 80
        self._height = 32
        
        # Initialize canvas
        super().__init__(master, highlightthickness=0, bg=master.cget('bg') if hasattr(master, 'cget') else THEME['bg_main'], **kwargs)
        
        # Calculate initial size (must be done after super().__init__)
        self._update_size()
        
        self.config(cursor='hand2')
        
        # Bind events
        self.bind('<Enter>', self._on_enter)
        self.bind('<Leave>', self._on_leave)
        self.bind('<ButtonPress-1>', self._on_press)
        self.bind('<ButtonRelease-1>', self._on_release)
        self.bind('<Configure>', self._on_configure)
        
        # Initial draw
        self.after(10, self._draw)
    
    def _update_size(self):
        """Calculate and set the button size based on text."""
        # Create a temporary font object to measure text
        import tkinter.font as tkFont
        font_obj = tkFont.Font(family=self.font[0], size=self.font[1])
        text_width = font_obj.measure(self.text)
        text_height = font_obj.metrics('linespace')
        
        if self.width:
            total_width = self.width * font_obj.measure('0')  # Approximate char width
        else:
            total_width = text_width + self.padx * 2
        
        total_height = text_height + self.pady * 2
        
        self.configure(width=total_width, height=total_height)
        self._width = total_width
        self._height = total_height
    
    def _draw(self):
        """Draw the rounded button."""
        self.delete('all')
        
        w = self.winfo_width()
        h = self.winfo_height()
        
        if w <= 1 or h <= 1:
            w = self._width
            h = self._height
        
        r = min(self.corner_radius, h // 2, w // 2)
        
        # Determine current color
        if self._state == 'disabled':
            fill_color = self._darken_color(self.bg_color, 0.5)
            text_color = self._darken_color(self.fg_color, 0.5)
        elif self._pressed:
            fill_color = self.active_color
            text_color = self.fg_color
        else:
            fill_color = getattr(self, '_current_bg', self.bg_color)
            text_color = self.fg_color
        
        # Draw rounded rectangle using polygon with arcs
        self._create_rounded_rect(2, 2, w-2, h-2, r, fill=fill_color, outline='')
        
        # Draw subtle border/shadow for depth
        self._create_rounded_rect(2, 2, w-2, h-2, r, fill='', outline=self._lighten_color(fill_color, 0.2), width=1)
        
        # Draw text
        self.create_text(w // 2, h // 2, text=self.text, fill=text_color, font=self.font, tags='text')
    
    def _create_rounded_rect(self, x1, y1, x2, y2, r, **kwargs):
        """Create a rounded rectangle on the canvas."""
        points = [
            x1 + r, y1,
            x2 - r, y1,
            x2, y1,
            x2, y1 + r,
            x2, y2 - r,
            x2, y2,
            x2 - r, y2,
            x1 + r, y2,
            x1, y2,
            x1, y2 - r,
            x1, y1 + r,
            x1, y1,
        ]
        return self.create_polygon(points, smooth=True, **kwargs)
    
    def _lighten_color(self, color, factor):
        """Lighten a hex color by a factor."""
        try:
            color = color.lstrip('#')
            r = int(color[0:2], 16)
            g = int(color[2:4], 16)
            b = int(color[4:6], 16)
            r = min(255, int(r + (255 - r) * factor))
            g = min(255, int(g + (255 - g) * factor))
            b = min(255, int(b + (255 - b) * factor))
            return f'#{r:02x}{g:02x}{b:02x}'
        except:
            return color
    
    def _darken_color(self, color, factor):
        """Darken a hex color by a factor."""
        try:
            color = color.lstrip('#')
            r = int(int(color[0:2], 16) * factor)
            g = int(int(color[2:4], 16) * factor)
            b = int(int(color[4:6], 16) * factor)
            return f'#{r:02x}{g:02x}{b:02x}'
        except:
            return color
    
    def _on_enter(self, event):
        if self._state != 'disabled':
            self._current_bg = self.hover_color
            self._draw()
    
    def _on_leave(self, event):
        if self._state != 'disabled':
            self._current_bg = self.bg_color
            self._pressed = False
            self._draw()
    
    def _on_press(self, event):
        if self._state != 'disabled':
            self._pressed = True
            self._draw()
    
    def _on_release(self, event):
        if self._state != 'disabled' and self._pressed:
            self._pressed = False
            self._draw()
            if self.command:
                self.command()
    
    def _on_configure(self, event):
        self._draw()

    def apply_theme(self):
        self.bg_color = THEME['button_bg']
        self.fg_color = THEME['button_fg']
        self.hover_color = THEME['button_hover']
        self.active_color = THEME['button_highlight']
        self.font = (THEME['font_family_sans'], THEME['font_size_main'])
        self._current_bg = self.bg_color
        tk.Canvas.configure(self, bg=self.master.cget('bg'))
        self._draw()

    def configure(self, **kwargs):
        """Handle configuration changes."""
        if 'text' in kwargs:
            self.text = kwargs.pop('text')
        if 'command' in kwargs:
            self.command = kwargs.pop('command')
        if 'state' in kwargs:
            self._state = kwargs.pop('state')
        if 'bg' in kwargs:
            self.bg_color = kwargs.pop('bg')
            self._current_bg = self.bg_color
        if 'fg' in kwargs:
            self.fg_color = kwargs.pop('fg')
        if 'activebackground' in kwargs:
            self.hover_color = kwargs.pop('activebackground')
        if 'font' in kwargs:
            self.font = kwargs.pop('font')
        
        super().configure(**kwargs)
        self._draw()
    
    config = configure
    
    def cget(self, key):
        """Get configuration value."""
        if key == 'text':
            return self.text
        elif key == 'state':
            return self._state
        elif key == 'bg':
            return self.bg_color
        elif key == 'fg':
            return self.fg_color
        return super().cget(key)
    
    def __getitem__(self, key):
        return self.cget(key)
    
    def __setitem__(self, key, value):
        self.configure(**{key: value})

class DarkEntry(tk.Frame):
    """Modern entry with rounded appearance."""
    def __init__(self, master, **kwargs):
        # Extract entry-specific kwargs
        self.textvariable = kwargs.pop('textvariable', None)
        entry_bg = kwargs.pop('bg', THEME['entry_bg'])
        entry_fg = kwargs.pop('fg', THEME['entry_fg'])
        entry_font = kwargs.pop('font', (THEME['font_family_sans'], THEME['font_size_main']))
        
        # Frame background matches entry for seamless look
        super().__init__(master, bg=entry_bg, **kwargs)
        
        # Configure rounded appearance using padx/pady on frame
        self.configure(highlightthickness=2, highlightbackground=THEME['border'], highlightcolor=THEME['accent'])
        
        # Inner entry widget
        self._entry = tk.Entry(self, bg=entry_bg, fg=entry_fg, insertbackground=THEME['fg_text'],
                               relief=tk.FLAT, bd=0, font=entry_font, textvariable=self.textvariable)
        self._entry.pack(fill=tk.BOTH, expand=True, padx=8, pady=6)
        
        # Bind focus events for accent color
        self._entry.bind('<FocusIn>', lambda e: self.configure(highlightbackground=THEME['accent']))
        self._entry.bind('<FocusOut>', lambda e: self.configure(highlightbackground=THEME['border']))
    
    def get(self):
        return self._entry.get()
    
    def delete(self, first, last=None):
        return self._entry.delete(first, last)
    
    def insert(self, index, string):
        return self._entry.insert(index, string)
    
    def configure(self, **kwargs):
        if 'bg' in kwargs:
            bg = kwargs.pop('bg')
            super().configure(bg=bg)
            self._entry.configure(bg=bg)
        if 'fg' in kwargs:
            self._entry.configure(fg=kwargs.pop('fg'))
        if 'font' in kwargs:
            self._entry.configure(font=kwargs.pop('font'))
        if 'insertbackground' in kwargs:
            self._entry.configure(insertbackground=kwargs.pop('insertbackground'))
        super().configure(**kwargs)

    config = configure

    def apply_theme(self):
        self.configure(bg=THEME['entry_bg'], fg=THEME['entry_fg'],
                       font=(THEME['font_family_sans'], THEME['font_size_main']),
                       insertbackground=THEME['fg_text'],
                       highlightbackground=THEME['border'], highlightcolor=THEME['accent'])

    def bind(self, sequence=None, func=None, add=None):
        """Bind to the inner entry widget."""
        return self._entry.bind(sequence, func, add)

class DarkListbox(tk.Frame):
    """Modern listbox with rounded border appearance."""
    def __init__(self, master, **kwargs):
        # Extract listbox-specific kwargs
        list_bg = kwargs.pop('bg', THEME['list_bg'])
        list_fg = kwargs.pop('fg', THEME['entry_fg'])
        select_bg = kwargs.pop('selectbackground', THEME['selection'])
        select_fg = kwargs.pop('selectforeground', THEME['fg_highlight'])
        list_font = kwargs.pop('font', (THEME['font_family_sans'], THEME['font_size_main']))
        height = kwargs.pop('height', 10)
        selectmode = kwargs.pop('selectmode', tk.SINGLE)
        
        # Frame for rounded border effect
        super().__init__(master, bg=list_bg, **kwargs)
        self.configure(highlightthickness=2, highlightbackground=THEME['border'], highlightcolor=THEME['accent'])
        
        # Inner listbox
        self._listbox = tk.Listbox(self, bg=list_bg, fg=list_fg, selectbackground=select_bg,
                                    selectforeground=select_fg, relief=tk.FLAT, bd=0,
                                    highlightthickness=0, font=list_font, activestyle='none',
                                    height=height, selectmode=selectmode)
        self._listbox.pack(fill=tk.BOTH, expand=True, padx=4, pady=4)
    
    # Delegate common listbox methods
    def insert(self, index, *elements):
        return self._listbox.insert(index, *elements)
    
    def delete(self, first, last=None):
        return self._listbox.delete(first, last)
    
    def get(self, first, last=None):
        return self._listbox.get(first, last)
    
    def curselection(self):
        return self._listbox.curselection()
    
    def selection_set(self, first, last=None):
        return self._listbox.selection_set(first, last)
    
    def selection_clear(self, first, last=None):
        return self._listbox.selection_clear(first, last)
    
    def see(self, index):
        return self._listbox.see(index)
    
    def yview(self, *args):
        return self._listbox.yview(*args)
    
    def yview_scroll(self, number, what):
        return self._listbox.yview_scroll(number, what)
    
    def bind(self, sequence=None, func=None, add=None):
        return self._listbox.bind(sequence, func, add)
    
    def configure(self, **kwargs):
        if 'yscrollcommand' in kwargs:
            self._listbox.configure(yscrollcommand=kwargs.pop('yscrollcommand'))
        if 'bg' in kwargs:
            bg = kwargs.pop('bg')
            super().configure(bg=bg)
            self._listbox.configure(bg=bg)
        if 'fg' in kwargs:
            self._listbox.configure(fg=kwargs.pop('fg'))
        if 'selectbackground' in kwargs:
            self._listbox.configure(selectbackground=kwargs.pop('selectbackground'))
        if 'selectforeground' in kwargs:
            self._listbox.configure(selectforeground=kwargs.pop('selectforeground'))
        if 'font' in kwargs:
            self._listbox.configure(font=kwargs.pop('font'))
        super().configure(**kwargs)
    
    config = configure

    def apply_theme(self):
        self.configure(bg=THEME['list_bg'], fg=THEME['entry_fg'],
                       selectbackground=THEME['selection'], selectforeground=THEME['fg_highlight'],
                       font=(THEME['font_family_sans'], THEME['font_size_main']),
                       highlightbackground=THEME['border'], highlightcolor=THEME['accent'])

    def size(self):
        return self._listbox.size()

class DarkScrollbar(ttk.Scrollbar):
    def __init__(self, master, **kwargs):
        super().__init__(master, **kwargs)

class DarkTreeview(tk.Frame):
    """Themed ttk.Treeview wrapped in a bordered frame (list replacement)."""
    def __init__(self, master, selectmode='browse', height=10, **kwargs):
        super().__init__(master, bg=THEME['list_bg'], **kwargs)
        self.configure(highlightthickness=2, highlightbackground=THEME['border'],
                       highlightcolor=THEME['accent'])
        self.tree = ttk.Treeview(self, show='tree', style='Dark.Treeview',
                                 selectmode=selectmode, height=height)
        self.tree.pack(fill=tk.BOTH, expand=True, padx=4, pady=4)

    def bind(self, sequence=None, func=None, add=None):
        return self.tree.bind(sequence, func, add)

    def yview(self, *args):
        return self.tree.yview(*args)

    def configure(self, **kwargs):
        if 'yscrollcommand' in kwargs:
            self.tree.configure(yscrollcommand=kwargs.pop('yscrollcommand'))
        super().configure(**kwargs)

    config = configure

    def apply_theme(self):
        tk.Frame.configure(self, bg=THEME['list_bg'],
                           highlightbackground=THEME['border'],
                           highlightcolor=THEME['accent'])

from collections import OrderedDict

class ThumbnailStrip(tk.Frame):
    """Virtualized horizontal filmstrip: only visible slots are drawn."""
    THUMB_W = 96
    THUMB_H = 54
    PAD = 6
    PHOTO_CACHE = 400  # PhotoImage LRU cap; evicted thumbs re-request cheaply

    def __init__(self, master, on_click=None, **kwargs):
        super().__init__(master, bg=THEME['bg_sidebar'], **kwargs)
        self.on_click = on_click            # callback(filename)
        self.request_thumbs = None          # callback(list_of_filenames)
        self.filenames = []
        self.current = None
        self.photos = OrderedDict()         # filename -> PhotoImage (LRU)
        self.slot_w = self.THUMB_W + self.PAD

        self.canvas = tk.Canvas(self, height=self.THUMB_H + 10, bg=THEME['bg_sidebar'],
                                highlightthickness=0, xscrollincrement=self.slot_w // 2)
        self.scrollbar = tk.Scrollbar(self, orient=tk.HORIZONTAL, command=self.canvas.xview)
        self.canvas.configure(xscrollcommand=self._on_scroll_set)
        self.canvas.pack(fill=tk.X, side=tk.TOP)
        self.scrollbar.pack(fill=tk.X, side=tk.BOTTOM)

        self.canvas.bind('<Configure>', lambda e: self._render())
        self.canvas.bind('<Button-1>', self._on_click_event)
        self.canvas.bind('<MouseWheel>', self._on_wheel)

    def set_items(self, filenames):
        self.filenames = list(filenames)
        width = max(1, len(self.filenames) * self.slot_w)
        self.canvas.configure(scrollregion=(0, 0, width, self.THUMB_H + 10))
        self.canvas.xview_moveto(0)
        self._render()

    def set_current(self, filename):
        self.current = filename
        if filename in self.photos:
            self.photos.move_to_end(filename)
        try:
            idx = self.filenames.index(filename)
        except ValueError:
            self._render()
            return
        first, last = self._visible_range()
        if idx <= first or idx >= last:
            total_w = max(1, len(self.filenames) * self.slot_w)
            target = max(0, idx * self.slot_w - self.canvas.winfo_width() // 2)
            self.canvas.xview_moveto(target / total_w)
        self._render()

    def set_thumbnail(self, filename, photo):
        self.photos[filename] = photo
        self.photos.move_to_end(filename)
        while len(self.photos) > self.PHOTO_CACHE:
            self.photos.popitem(last=False)
        first, last = self._visible_range()
        if filename in self.filenames[first:last + 1]:
            self._render()

    def clear_thumbnails(self):
        self.photos.clear()
        self._render()

    def _visible_range(self):
        if not self.filenames:
            return 0, -1
        x0 = self.canvas.canvasx(0)
        w = max(1, self.canvas.winfo_width())
        first = max(0, int(x0 // self.slot_w))
        last = min(len(self.filenames) - 1, int((x0 + w) // self.slot_w) + 1)
        return first, last

    def _render(self):
        self.canvas.delete('thumb')
        first, last = self._visible_range()
        if last < first:
            return
        needed = []
        y = 5
        for i in range(first, last + 1):
            fname = self.filenames[i]
            x = i * self.slot_w + self.PAD // 2
            photo = self.photos.get(fname)
            if photo is not None:
                self.canvas.create_image(x + self.THUMB_W // 2, y + self.THUMB_H // 2,
                                         image=photo, tags='thumb')
            else:
                self.canvas.create_rectangle(x, y, x + self.THUMB_W, y + self.THUMB_H,
                                             fill=THEME['entry_bg'], outline=THEME['border'],
                                             tags='thumb')
                needed.append(fname)
            if fname == self.current:
                self.canvas.create_rectangle(x - 2, y - 2, x + self.THUMB_W + 2, y + self.THUMB_H + 2,
                                             outline=THEME['selection'], width=3, tags='thumb')
        if needed and self.request_thumbs:
            self.request_thumbs(needed)

    def _on_scroll_set(self, lo, hi):
        self.scrollbar.set(lo, hi)
        self._render()

    def _on_click_event(self, event):
        if not self.filenames:
            return
        idx = int(self.canvas.canvasx(event.x) // self.slot_w)
        if 0 <= idx < len(self.filenames) and self.on_click:
            self.on_click(self.filenames[idx])

    def _on_wheel(self, event):
        self.canvas.xview_scroll(-1 if event.delta > 0 else 1, 'units')

    def apply_theme(self):
        tk.Frame.configure(self, bg=THEME['bg_sidebar'])
        self.canvas.configure(bg=THEME['bg_sidebar'])
        self._render()

class Tooltip:
    """Delayed hover tooltip for any widget."""
    def __init__(self, widget, text, delay=600):
        self.widget = widget
        self.text = text
        self.delay = delay
        self._after_id = None
        self._tip = None
        widget.bind('<Enter>', self._schedule, add='+')
        widget.bind('<Leave>', self._hide, add='+')
        widget.bind('<ButtonPress>', self._hide, add='+')

    def _schedule(self, event=None):
        self._cancel()
        self._after_id = self.widget.after(self.delay, self._show)

    def _cancel(self):
        if self._after_id:
            self.widget.after_cancel(self._after_id)
            self._after_id = None

    def _show(self):
        if self._tip:
            return
        x = self.widget.winfo_rootx() + 10
        y = self.widget.winfo_rooty() + self.widget.winfo_height() + 6
        self._tip = tk.Toplevel(self.widget)
        self._tip.wm_overrideredirect(True)
        self._tip.wm_geometry(f"+{x}+{y}")
        tk.Label(self._tip, text=self.text, bg=THEME['bg_sidebar'], fg=THEME['fg_text'],
                 relief=tk.SOLID, borderwidth=1, padx=6, pady=3,
                 font=(THEME['font_family_sans'], THEME['font_size_main']-1)).pack()

    def _hide(self, event=None):
        self._cancel()
        if self._tip:
            self._tip.destroy()
            self._tip = None

class DarkProgressBar(tk.Canvas):
    """Modern progress bar with rounded corners."""
    def __init__(self, master, **kwargs):
        self.height = kwargs.pop('height', 24)
        self.bg_color = kwargs.pop('bg', THEME['entry_bg'])
        self.fg_color = kwargs.pop('fg', THEME['button_highlight'])
        self.text_color = kwargs.pop('text_color', THEME['fg_text'])
        self.corner_radius = kwargs.pop('corner_radius', 10)
        
        # Get parent bg for seamless look
        parent_bg = master.cget('bg') if hasattr(master, 'cget') else THEME['bg_sidebar']
        
        super().__init__(master, height=self.height, bg=parent_bg, highlightthickness=0, **kwargs)
        
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
    
    def _create_rounded_rect(self, x1, y1, x2, y2, r, **kwargs):
        """Create a rounded rectangle."""
        points = [
            x1 + r, y1, x2 - r, y1, x2, y1, x2, y1 + r,
            x2, y2 - r, x2, y2, x2 - r, y2, x1 + r, y2,
            x1, y2, x1, y2 - r, x1, y1 + r, x1, y1,
        ]
        return self.create_polygon(points, smooth=True, **kwargs)
    
    def _draw(self):
        """Redraw the progress bar with rounded corners."""
        self.delete("all")
        
        width = self.winfo_width()
        height = self.winfo_height()
        
        if width <= 1:
            return
        
        r = min(self.corner_radius, height // 2)
        padding = 2
        
        # Draw background track
        self._create_rounded_rect(padding, padding, width - padding, height - padding, r,
                                   fill=self.bg_color, outline=THEME['border'])
        
        # Calculate fill width
        if self._max_value > 0:
            fill_ratio = self._value / self._max_value
            fill_width = (width - padding * 2) * fill_ratio
        else:
            fill_width = 0
        
        # Draw progress fill with rounded corners
        if fill_width > r * 2:
            self._create_rounded_rect(padding, padding, padding + fill_width, height - padding, r,
                                       fill=self.fg_color, outline='')
        elif fill_width > 0:
            # For small fill, just draw a smaller rounded rect
            self._create_rounded_rect(padding, padding, padding + max(fill_width, r), height - padding, 
                                       min(r, fill_width / 2), fill=self.fg_color, outline='')
        
        # Draw text centered
        if self._text:
            self.create_text(width / 2, height / 2, text=self._text,
                           fill=self.text_color, font=(THEME['font_family_sans'], THEME['font_size_main']-1, 'bold'))
    
    def _on_resize(self, event):
        """Handle resize events."""
        self._draw()

    def apply_theme(self):
        self.bg_color = THEME['entry_bg']
        self.fg_color = THEME['button_highlight']
        self.text_color = THEME['fg_text']
        tk.Canvas.configure(self, bg=self.master.cget('bg'))
        self._draw()


class ProgressDialog(tk.Toplevel):
    """Modal progress dialog for long-running background tasks."""
    def __init__(self, master, title="Working..."):
        super().__init__(master)
        self.title(title)
        self.geometry("420x120")
        self.configure(bg=THEME['bg_main'])
        self.resizable(False, False)
        self.transient(master)
        self.grab_set()
        # The task cannot be interrupted safely, so ignore close attempts
        self.protocol("WM_DELETE_WINDOW", lambda: None)

        self.label = DarkLabel(self, text="Starting...", bg=THEME['bg_main'])
        self.label.pack(pady=(20, 8), padx=16, fill=tk.X)

        self.bar = DarkProgressBar(self, height=22)
        self.bar.pack(fill=tk.X, padx=16, pady=(0, 16))

    def update_progress(self, current, total, text=""):
        if total:
            percent = int(current / total * 100)
            self.bar.set_value(current, total, f"{percent}%")
        if text:
            self.label.config(text=text)

    def close(self):
        try:
            self.grab_release()
        except tk.TclError:
            pass
        self.destroy()

