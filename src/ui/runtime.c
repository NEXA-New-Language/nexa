/* NEXA UI 1.0 native runtime (spec section 8).
 *
 * This is the runtime the generated x86-64 code links against. It is a
 * native runtime: it does not translate the UI to HTML, CSS or JavaScript
 * and it does not run in a browser.
 *
 * Two backends:
 *  - Windowed (GTK 3): used when a display is available (DISPLAY or
 *    WAYLAND_DISPLAY set) and the program was linked with GTK. The
 *    generated render function builds real native widgets; every event
 *    re-renders in place.
 *  - Headless: used otherwise, and forced with NEXA_UI_HEADLESS=1. The
 *    render function prints a deterministic snapshot of the UI tree to
 *    stdout, and the runtime reads event commands from stdin:
 *        click <button label>
 *        type <field key> <value>
 *        toggle <checkbox key>
 *        help | quit
 *    After each event the UI is re-rendered, so state changes are visible
 *    in the next snapshot. This backend is what tests and CI use.
 *
 * GTK is used through hand-written declarations (no GTK headers are
 * required) and linked with -lgtk-3 etc. When GTK is not available at
 * link time the program is built with -DNEXA_UI_NO_GTK and always runs
 * headless.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

/* NEXA strings carry a 24-byte header (len, cap, shared) before the
 * NUL-terminated data; a string value is a pointer to the data. Strings
 * handed in from the UI are always shared: NEXA code must never append
 * to them in place. */
static char *nexa_dup_string(const char *s) {
    size_t len = strlen(s);
    char *block = malloc(24 + len + 1);
    if (!block) {
        fprintf(stderr, "out of memory\n");
        exit(1);
    }
    *(int64_t *)block = (int64_t)len;
    *(int64_t *)(block + 8) = (int64_t)len;
    *(int64_t *)(block + 16) = 1;
    memcpy(block + 24, s, len + 1);
    return block + 24;
}

/* State storage emitted by the generated code. */
extern long long __nexa_ui_state[];

enum { MODE_HEADLESS = 0, MODE_GTK = 1 };

static void (*g_render)(void) = NULL;
static const char *g_app_name = "NEXA";
static int g_mode = MODE_HEADLESS;
static int g_building = 0;
static int g_updating = 0;

#ifndef NEXA_UI_NO_GTK
/* GTK 3 declarations (stable C API; no headers needed). */
extern void gtk_init(int *argc, char ***argv);
extern void *gtk_window_new(int type);
extern void gtk_window_set_title(void *window, const char *title);
extern void gtk_window_set_default_size(void *window, int width, int height);
extern void gtk_container_add(void *container, void *widget);
extern void gtk_widget_show_all(void *widget);
extern void gtk_main(void);
extern void gtk_main_quit(void);
extern void *gtk_box_new(int orientation, int spacing);
extern void gtk_box_pack_start(void *box, void *child, int expand, int fill,
                               unsigned int padding);
extern void gtk_box_set_spacing(void *box, int spacing);
extern void *gtk_label_new(const char *str);
extern void gtk_label_set_text(void *label, const char *str);
extern void *gtk_button_new_with_label(const char *label);
extern void gtk_button_set_label(void *button, const char *label);
extern void *gtk_entry_new(void);
extern void gtk_entry_set_text(void *entry, const char *text);
extern const char *gtk_entry_get_text(void *entry);
extern void *gtk_check_button_new_with_label(const char *label);
extern int gtk_toggle_button_get_active(void *toggle_button);
extern void gtk_toggle_button_set_active(void *toggle_button, int is_active);
extern void *gtk_separator_new(int orientation);
extern void *gtk_image_new_from_file(const char *filename);
extern void gtk_widget_set_size_request(void *widget, int width, int height);
extern void gtk_widget_set_margin_top(void *widget, int margin);
extern void gtk_widget_set_margin_bottom(void *widget, int margin);
extern void gtk_widget_set_margin_start(void *widget, int margin);
extern void gtk_widget_set_margin_end(void *widget, int margin);
extern void gtk_widget_set_halign(void *widget, int align);
extern void gtk_widget_set_valign(void *widget, int align);
extern void gtk_widget_set_visible(void *widget, int visible);
extern void gtk_container_set_border_width(void *container,
                                           unsigned int border_width);
extern unsigned long g_signal_connect_data(void *instance,
                                           const char *detailed_signal,
                                           void *c_handler, void *data,
                                           void *destroy_data,
                                           int connect_flags);
extern void gtk_widget_override_background_color(void *widget, int state,
                                                 const void *rgba);

typedef struct {
    double red, green, blue, alpha;
} GdkRGBA;

#define GTK_WINDOW_TOPLEVEL 0
#define GTK_ORIENTATION_HORIZONTAL 0
#define GTK_ORIENTATION_VERTICAL 1
#define GTK_ALIGN_FILL 0
#define GTK_ALIGN_START 1
#define GTK_ALIGN_END 2
#define GTK_ALIGN_CENTER 3
#define GTK_STATE_FLAG_NORMAL 0

/* ---- GTK widget bookkeeping (update-in-place rendering) ---- */
enum { WK_LAYOUT = 0, WK_TEXT, WK_BUTTON, WK_TEXTFIELD, WK_CHECKBOX,
       WK_SPACER, WK_DIVIDER, WK_IMAGE };

typedef struct {
    int kind;
    void *widget;
    void *handler;
    long long w;
    long long h;
} WidgetSlot;

#define MAX_WIDGETS 512
static WidgetSlot g_slots[MAX_WIDGETS];
static int g_slot_count = 0;
static int g_slot_index = 0;

static void *g_window = NULL;
static void *g_root = NULL;
static long long g_win_w = 800;
static long long g_win_h = 600;
static void *g_last_widget = NULL;

static void *g_stack[64];
static int g_stack_top = 0;

static void push_container(void *box) {
    if (g_stack_top < 64) {
        g_stack[g_stack_top++] = box;
    }
}

static void pop_container(void) {
    if (g_stack_top > 0) {
        g_stack_top--;
    }
}

static void pack_widget(void *widget) {
    if (g_stack_top > 0) {
        gtk_box_pack_start(g_stack[g_stack_top - 1], widget, 0, 0, 0);
    }
}

static WidgetSlot *next_slot(int kind, void *widget) {
    if (g_building) {
        if (g_slot_count >= MAX_WIDGETS) {
            return NULL;
        }
        WidgetSlot *slot = &g_slots[g_slot_count++];
        slot->kind = kind;
        slot->widget = widget;
        slot->handler = NULL;
        slot->w = -1;
        slot->h = -1;
        return slot;
    }
    if (g_slot_index >= g_slot_count) {
        return NULL;
    }
    return &g_slots[g_slot_index++];
}

static WidgetSlot *find_slot(void *widget) {
    for (int i = 0; i < g_slot_count; i++) {
        if (g_slots[i].widget == widget) {
            return &g_slots[i];
        }
    }
    return NULL;
}

static void on_destroy(void *widget, void *data) {
    (void)widget;
    (void)data;
    gtk_main_quit();
}

static void on_button_clicked(void *widget, void *data) {
    (void)widget;
    void (*handler)(void) = (void (*)(void))data;
    if (handler) {
        handler();
    }
    if (g_render) {
        g_render();
    }
}

static void on_entry_changed(void *entry, void *data) {
    if (g_updating) {
        return;
    }
    const char *text = gtk_entry_get_text(entry);
    void (*handler)(const char *) = (void (*)(const char *))data;
    if (handler) {
        handler(nexa_dup_string(text ? text : ""));
    }
    if (g_render) {
        g_render();
    }
}

static void on_check_toggled(void *button, void *data) {
    if (g_updating) {
        return;
    }
    int active = gtk_toggle_button_get_active(button);
    void (*handler)(int) = (void (*)(int))data;
    if (handler) {
        handler(active);
    }
    if (g_render) {
        g_render();
    }
}

static void ui_text_value(const char *text) {
    if (g_building) {
        void *label = gtk_label_new(text ? text : "");
        pack_widget(label);
        next_slot(WK_TEXT, label);
        g_last_widget = label;
    } else {
        WidgetSlot *slot = next_slot(WK_TEXT, NULL);
        if (slot && slot->widget) {
            gtk_label_set_text(slot->widget, text ? text : "");
            g_last_widget = slot->widget;
        }
    }
}

static void apply_margins(void *widget, int value) {
    gtk_widget_set_margin_top(widget, value);
    gtk_widget_set_margin_bottom(widget, value);
    gtk_widget_set_margin_start(widget, value);
    gtk_widget_set_margin_end(widget, value);
}

static void apply_int_prop(const char *name, long long v) {
    if (!g_last_widget) {
        return;
    }
    if (g_last_widget == g_window) {
        if (strcmp(name, "width") == 0) {
            g_win_w = v;
        } else if (strcmp(name, "height") == 0) {
            g_win_h = v;
        } else {
            return;
        }
        gtk_window_set_default_size(g_window, (int)g_win_w, (int)g_win_h);
        return;
    }
    WidgetSlot *slot = find_slot(g_last_widget);
    if (strcmp(name, "width") == 0) {
        if (slot) {
            slot->w = v;
        }
        gtk_widget_set_size_request(g_last_widget, (int)v,
                                    slot && slot->h >= 0 ? (int)slot->h : -1);
    } else if (strcmp(name, "height") == 0) {
        if (slot) {
            slot->h = v;
        }
        gtk_widget_set_size_request(g_last_widget,
                                    slot && slot->w >= 0 ? (int)slot->w : -1,
                                    (int)v);
    } else if (strcmp(name, "spacing") == 0) {
        if (slot && slot->kind == WK_LAYOUT) {
            gtk_box_set_spacing(g_last_widget, (int)v);
        }
    } else if (strcmp(name, "padding") == 0) {
        if (slot && slot->kind == WK_LAYOUT) {
            gtk_container_set_border_width(g_last_widget, (unsigned)v);
        } else {
            apply_margins(g_last_widget, (int)v);
        }
    } else if (strcmp(name, "margin") == 0) {
        apply_margins(g_last_widget, (int)v);
    }
}
#endif /* !NEXA_UI_NO_GTK */

/* ---- Headless registry: interactive nodes, keyed for stdin events ---- */
enum { HK_BUTTON = 0, HK_TEXTFIELD, HK_CHECKBOX };

typedef struct {
    int kind;
    const char *key;
    void *handler;
    long long state_slot;
} HeadlessEntry;

#define MAX_HEADLESS 256
static HeadlessEntry g_headless[MAX_HEADLESS];
static int g_headless_count = 0;

static HeadlessEntry *find_headless(int kind, const char *key) {
    for (int i = 0; i < g_headless_count; i++) {
        if (g_headless[i].kind == kind && strcmp(g_headless[i].key, key) == 0) {
            return &g_headless[i];
        }
    }
    return NULL;
}

static void register_headless(int kind, const char *key, void *handler,
                              long long state_slot) {
    if (g_headless_count >= MAX_HEADLESS) {
        return;
    }
    g_headless[g_headless_count].kind = kind;
    g_headless[g_headless_count].key = key ? key : "";
    g_headless[g_headless_count].handler = handler;
    g_headless[g_headless_count].state_slot = state_slot;
    g_headless_count++;
}

/* ---- Public API used by generated code ---- */

void nexa_ui_init(void (*render)(void), const char *app_name) {
    g_render = render;
    if (app_name) {
        g_app_name = app_name;
    }
    const char *headless = getenv("NEXA_UI_HEADLESS");
    if (headless && strcmp(headless, "1") == 0) {
        g_mode = MODE_HEADLESS;
        return;
    }
#ifdef NEXA_UI_NO_GTK
    g_mode = MODE_HEADLESS;
    if (getenv("DISPLAY") || getenv("WAYLAND_DISPLAY")) {
        fprintf(stderr,
                "NEXA UI: windowed backend is unavailable in this build; "
                "running headless.\n");
    }
#else
    if (getenv("DISPLAY") || getenv("WAYLAND_DISPLAY")) {
        g_mode = MODE_GTK;
        gtk_init(NULL, NULL);
    } else {
        g_mode = MODE_HEADLESS;
    }
#endif
}

void nexa_ui_run(void) {
    if (g_mode == MODE_GTK) {
#ifndef NEXA_UI_NO_GTK
        g_building = 1;
        if (g_render) {
            g_render();
        }
        g_building = 0;
        if (g_window) {
            gtk_widget_show_all(g_window);
        }
        gtk_main();
#endif
        return;
    }

    /* Headless event loop: snapshot, then one command per stdin line. */
    if (g_render) {
        g_render();
    }
    fflush(stdout);
    char line[4096];
    for (;;) {
        if (!fgets(line, sizeof line, stdin)) {
            break;
        }
        size_t len = strlen(line);
        while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r')) {
            line[--len] = '\0';
        }
        if (len == 0) {
            continue;
        }
        if (strcmp(line, "quit") == 0 || strcmp(line, "exit") == 0) {
            break;
        }
        if (strcmp(line, "help") == 0) {
            fputs("Commands: click <label> | type <key> <value> | toggle <key> | quit\n",
                  stdout);
            fflush(stdout);
            continue;
        }
        if (strncmp(line, "click ", 6) == 0) {
            const char *key = line + 6;
            HeadlessEntry *e = find_headless(HK_BUTTON, key);
            if (!e) {
                fprintf(stderr, "Unknown button: %s\n", key);
                continue;
            }
            void (*handler)(void) = (void (*)(void))e->handler;
            if (handler) {
                handler();
            }
            if (g_render) {
                g_render();
            }
            fflush(stdout);
        } else if (strncmp(line, "type ", 5) == 0) {
            char *rest = line + 5;
            char *sp = strchr(rest, ' ');
            if (!sp) {
                fprintf(stderr, "Usage: type <key> <value>\n");
                continue;
            }
            *sp = '\0';
            const char *value = sp + 1;
            HeadlessEntry *e = find_headless(HK_TEXTFIELD, rest);
            if (!e) {
                fprintf(stderr, "Unknown text field: %s\n", rest);
                continue;
            }
            void (*handler)(const char *) = (void (*)(const char *))e->handler;
            if (handler) {
                handler(nexa_dup_string(value));
            }
            if (g_render) {
                g_render();
            }
            fflush(stdout);
        } else if (strncmp(line, "toggle ", 7) == 0) {
            const char *key = line + 7;
            HeadlessEntry *e = find_headless(HK_CHECKBOX, key);
            if (!e) {
                fprintf(stderr, "Unknown checkbox: %s\n", key);
                continue;
            }
            long long current = 0;
            if (e->state_slot >= 0) {
                current = __nexa_ui_state[e->state_slot];
            }
            void (*handler)(int) = (void (*)(int))e->handler;
            if (handler) {
                handler((int)(current ^ 1));
            }
            if (g_render) {
                g_render();
            }
            fflush(stdout);
        } else {
            fprintf(stderr, "Unknown UI command: %s\n", line);
        }
    }
}

void nexa_ui_begin_render(void) {
    g_headless_count = 0;
    g_slot_index = 0;
    g_stack_top = 0;
}

void nexa_ui_window(const char *title, int w, int h) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    g_win_w = w;
    g_win_h = h;
    if (g_building) {
        g_window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
        gtk_window_set_title(g_window, title ? title : g_app_name);
        gtk_window_set_default_size(g_window, w, h);
        g_signal_connect_data(g_window, "destroy", (void *)on_destroy, NULL,
                              NULL, 0);
        g_root = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
        gtk_container_add(g_window, g_root);
        g_stack_top = 0;
        push_container(g_root);
        g_last_widget = g_window;
    } else {
        if (g_window) {
            gtk_window_set_title(g_window, title ? title : g_app_name);
            gtk_window_set_default_size(g_window, w, h);
        }
        g_stack_top = 0;
        if (g_root) {
            push_container(g_root);
        }
        g_last_widget = g_window;
    }
#endif
}

void nexa_ui_begin_layout(int kind) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    if (g_building) {
        void *box = gtk_box_new(kind == 1 ? GTK_ORIENTATION_HORIZONTAL
                                          : GTK_ORIENTATION_VERTICAL,
                                0);
        pack_widget(box);
        next_slot(WK_LAYOUT, box);
        push_container(box);
        g_last_widget = box;
    } else {
        WidgetSlot *slot = next_slot(WK_LAYOUT, NULL);
        if (slot && slot->widget) {
            push_container(slot->widget);
            g_last_widget = slot->widget;
        }
    }
#endif
}

void nexa_ui_end_layout(void) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    pop_container();
#endif
}

void nexa_ui_text_str(const char *s) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    ui_text_value(s ? s : "");
#endif
}

void nexa_ui_text_int(long long v) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    char buf[64];
    snprintf(buf, sizeof buf, "%lld", v);
    ui_text_value(buf);
#endif
}

void nexa_ui_text_float(double v) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    char buf[64];
    snprintf(buf, sizeof buf, "%.17g", v);
    ui_text_value(buf);
#endif
}

void nexa_ui_text_bool(int v) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    ui_text_value(v ? "true" : "false");
#endif
}

void nexa_ui_button(const char *label, void (*handler)(void)) {
    if (g_mode == MODE_GTK) {
#ifndef NEXA_UI_NO_GTK
        if (g_building) {
            void *btn = gtk_button_new_with_label(label ? label : "");
            pack_widget(btn);
            WidgetSlot *slot = next_slot(WK_BUTTON, btn);
            if (slot) {
                slot->handler = (void *)handler;
            }
            if (handler) {
                g_signal_connect_data(btn, "clicked",
                                      (void *)on_button_clicked,
                                      (void *)handler, NULL, 0);
            }
            g_last_widget = btn;
        } else {
            WidgetSlot *slot = next_slot(WK_BUTTON, NULL);
            if (slot && slot->widget) {
                gtk_button_set_label(slot->widget, label ? label : "");
                g_last_widget = slot->widget;
            }
        }
#endif
        return;
    }
    register_headless(HK_BUTTON, label, (void *)handler, -1);
}

void nexa_ui_textfield(const char *key, const char *value,
                       void (*handler)(const char *)) {
    if (g_mode == MODE_GTK) {
#ifndef NEXA_UI_NO_GTK
        if (g_building) {
            void *entry = gtk_entry_new();
            gtk_entry_set_text(entry, value ? value : "");
            pack_widget(entry);
            WidgetSlot *slot = next_slot(WK_TEXTFIELD, entry);
            if (slot) {
                slot->handler = (void *)handler;
            }
            if (handler) {
                g_signal_connect_data(entry, "changed",
                                      (void *)on_entry_changed,
                                      (void *)handler, NULL, 0);
            }
            g_last_widget = entry;
        } else {
            WidgetSlot *slot = next_slot(WK_TEXTFIELD, NULL);
            if (slot && slot->widget) {
                const char *cur = gtk_entry_get_text(slot->widget);
                const char *want = value ? value : "";
                if (!cur || strcmp(cur, want) != 0) {
                    g_updating = 1;
                    gtk_entry_set_text(slot->widget, want);
                    g_updating = 0;
                }
                g_last_widget = slot->widget;
            }
        }
#endif
        return;
    }
    register_headless(HK_TEXTFIELD, key, (void *)handler, -1);
}

void nexa_ui_checkbox(const char *key, int checked, void (*handler)(int),
                      long long state_slot) {
    if (g_mode == MODE_GTK) {
#ifndef NEXA_UI_NO_GTK
        if (g_building) {
            void *btn = gtk_check_button_new_with_label(key ? key : "");
            gtk_toggle_button_set_active(btn, checked ? 1 : 0);
            pack_widget(btn);
            WidgetSlot *slot = next_slot(WK_CHECKBOX, btn);
            if (slot) {
                slot->handler = (void *)handler;
            }
            if (handler) {
                g_signal_connect_data(btn, "toggled",
                                      (void *)on_check_toggled,
                                      (void *)handler, NULL, 0);
            }
            g_last_widget = btn;
        } else {
            WidgetSlot *slot = next_slot(WK_CHECKBOX, NULL);
            if (slot && slot->widget) {
                int cur = gtk_toggle_button_get_active(slot->widget);
                if (cur != (checked ? 1 : 0)) {
                    g_updating = 1;
                    gtk_toggle_button_set_active(slot->widget,
                                                 checked ? 1 : 0);
                    g_updating = 0;
                }
                g_last_widget = slot->widget;
            }
        }
#endif
        return;
    }
    register_headless(HK_CHECKBOX, key, (void *)handler, state_slot);
}

void nexa_ui_spacer(void) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    if (g_building) {
        void *box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
        gtk_widget_set_size_request(box, -1, 12);
        pack_widget(box);
        next_slot(WK_SPACER, box);
        g_last_widget = box;
    } else {
        WidgetSlot *slot = next_slot(WK_SPACER, NULL);
        if (slot && slot->widget) {
            g_last_widget = slot->widget;
        }
    }
#endif
}

void nexa_ui_divider(void) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    if (g_building) {
        void *sep = gtk_separator_new(GTK_ORIENTATION_HORIZONTAL);
        pack_widget(sep);
        next_slot(WK_DIVIDER, sep);
        g_last_widget = sep;
    } else {
        WidgetSlot *slot = next_slot(WK_DIVIDER, NULL);
        if (slot && slot->widget) {
            g_last_widget = slot->widget;
        }
    }
#endif
}

void nexa_ui_image(const char *path) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    if (g_building) {
        void *widget;
        FILE *f = path ? fopen(path, "rb") : NULL;
        if (f) {
            fclose(f);
            widget = gtk_image_new_from_file(path);
        } else {
            char buf[512];
            snprintf(buf, sizeof buf, "Image: %s", path ? path : "");
            widget = gtk_label_new(buf);
        }
        pack_widget(widget);
        next_slot(WK_IMAGE, widget);
        g_last_widget = widget;
    } else {
        WidgetSlot *slot = next_slot(WK_IMAGE, NULL);
        if (slot && slot->widget) {
            g_last_widget = slot->widget;
        }
    }
#endif
}

void nexa_ui_prop_int(const char *name, long long v) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    apply_int_prop(name, v);
#endif
}

void nexa_ui_prop_float(const char *name, double v) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    apply_int_prop(name, (long long)v);
#endif
}

void nexa_ui_prop_str(const char *name, const char *s) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    if (!g_last_widget || !name) {
        return;
    }
    if (strcmp(name, "background") == 0 && s && s[0] == '#' && strlen(s) == 7) {
        unsigned int rgb = (unsigned int)strtoul(s + 1, NULL, 16);
        GdkRGBA rgba;
        rgba.red = ((rgb >> 16) & 0xff) / 255.0;
        rgba.green = ((rgb >> 8) & 0xff) / 255.0;
        rgba.blue = (rgb & 0xff) / 255.0;
        rgba.alpha = 1.0;
        gtk_widget_override_background_color(g_last_widget,
                                             GTK_STATE_FLAG_NORMAL, &rgba);
    } else if (strcmp(name, "align") == 0 && s) {
        if (strcmp(s, "left") == 0) {
            gtk_widget_set_halign(g_last_widget, GTK_ALIGN_START);
        } else if (strcmp(s, "right") == 0) {
            gtk_widget_set_halign(g_last_widget, GTK_ALIGN_END);
        } else if (strcmp(s, "center") == 0 || strcmp(s, "middle") == 0) {
            gtk_widget_set_halign(g_last_widget, GTK_ALIGN_CENTER);
            gtk_widget_set_valign(g_last_widget, GTK_ALIGN_CENTER);
        } else if (strcmp(s, "top") == 0) {
            gtk_widget_set_valign(g_last_widget, GTK_ALIGN_START);
        } else if (strcmp(s, "bottom") == 0) {
            gtk_widget_set_valign(g_last_widget, GTK_ALIGN_END);
        }
    }
#endif
}

void nexa_ui_prop_bool(const char *name, int v) {
    if (g_mode != MODE_GTK) {
        return;
    }
#ifndef NEXA_UI_NO_GTK
    if (g_last_widget && name && strcmp(name, "visible") == 0) {
        gtk_widget_set_visible(g_last_widget, v ? 1 : 0);
    }
#endif
}

/* ---- Snapshot printing (headless backend; no-ops when windowed) ---- */

void nexa_ui_indent(int depth) {
    if (g_mode == MODE_GTK) {
        return;
    }
    for (int i = 0; i < depth; i++) {
        fputs("  ", stdout);
    }
}

void nexa_ui_print_str(const char *s) {
    if (g_mode == MODE_GTK) {
        return;
    }
    fputs(s ? s : "", stdout);
}

void nexa_ui_print_int(long long v) {
    if (g_mode == MODE_GTK) {
        return;
    }
    printf("%lld", v);
}

void nexa_ui_print_float(double v) {
    if (g_mode == MODE_GTK) {
        return;
    }
    printf("%.17g", v);
}

void nexa_ui_print_bool(int v) {
    if (g_mode == MODE_GTK) {
        return;
    }
    fputs(v ? "true" : "false", stdout);
}

void nexa_ui_print_nl(void) {
    if (g_mode == MODE_GTK) {
        return;
    }
    putchar('\n');
}
