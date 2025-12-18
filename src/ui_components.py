import tkinter as tk
from tkinter import ttk

# Eclipse Theme Colors - Standard Tkinter Compatible
THEME = {
    'bg_main': '#0f0c29',       # Main Window Background
    'bg_sidebar': '#1a1a2e',    # Sidebar/Panel Background
    'fg_text': '#e0e0e0',       # Text Color
    'fg_highlight': '#ffffff',  # White text
    'accent': '#3a0ca3',        # Button Base
    'selection': '#4834d4',     # Selection Purple
    'border': '#2d3436',        # Dark border
    'button_bg': '#3a0ca3',     # Deep Purple Button
    'button_fg': '#ffffff',     # White Text
    'button_hover': '#4361ee',  # Lighter Purple Hover
    'entry_bg': '#16213e',      # Dark Input Background
    'entry_fg': '#ffffff',      # White Input Text
    'list_bg': '#16213e',       # Listbox Background
    'button_highlight': '#4cc9f0' # Neon Cyan for active
}

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
        kwargs.setdefault('font', ('Segoe UI', 10))
        super().__init__(master, **kwargs)

class SectionLabel(tk.Label):
    def __init__(self, master, **kwargs):
        kwargs.setdefault('bg', THEME['bg_sidebar'])
        kwargs.setdefault('fg', THEME['button_highlight'])  # Neon cyan for headers
        kwargs.setdefault('font', ('Segoe UI', 11, 'bold'))
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
        kwargs.setdefault('font', ('Segoe UI', 10))
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
        kwargs.setdefault('font', ('Segoe UI', 10))
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
        kwargs.setdefault('font', ('Segoe UI', 10))
        kwargs.setdefault('activestyle', 'none')
        super().__init__(master, **kwargs)

class DarkScrollbar(ttk.Scrollbar):
    def __init__(self, master, **kwargs):
        super().__init__(master, **kwargs)
