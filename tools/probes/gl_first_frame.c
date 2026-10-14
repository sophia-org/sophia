#include <EGL/egl.h>
#include <GL/gl.h>
#include <GL/glx.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { WIDTH = 300, HEIGHT = 300 };
static int x_error;

static int record_error(Display *display, XErrorEvent *event) {
    (void)display;
    fprintf(stderr, "gl_first_frame x_error=%u major=%u minor=%u\n",
            event->error_code, event->request_code, event->minor_code);
    x_error = 1;
    return 0;
}

static void require(int condition, const char *stage) {
    if (!condition) {
        fprintf(stderr, "gl_first_frame failed=%s\n", stage);
        exit(1);
    }
}

static Window window(Display *display, XVisualInfo *visual, Colormap *colormap) {
    *colormap = XCreateColormap(display, DefaultRootWindow(display), visual->visual, AllocNone);
    XSetWindowAttributes attributes = {
        .colormap = *colormap,
        .override_redirect = True,
    };
    /* Keep probe geometry independent of window-manager layout policy. */
    Window created = XCreateWindow(display, DefaultRootWindow(display), 0, 0, WIDTH, HEIGHT,
                                   0, visual->depth, InputOutput, visual->visual,
                                   CWColormap | CWOverrideRedirect, &attributes);
    XMapWindow(display, created);
    XSync(display, False);
    require(created && !x_error, "window");
    return created;
}

static void draw(void) {
    printf("gl_first_frame renderer=%s\n", glGetString(GL_RENDERER));
    const unsigned char colors[3][3] = {{0x21, 0x43, 0x65}, {0xab, 0xcd, 0xef}, {0x11, 0x99, 0xdd}};
    glDisable(GL_DITHER);
    glViewport(0, 0, WIDTH, HEIGHT);
    glEnable(GL_SCISSOR_TEST);
    for (int stripe = 0; stripe < 3; ++stripe) {
        glScissor(stripe * (WIDTH / 3), 0, WIDTH / 3, HEIGHT);
        glClearColor(colors[stripe][0] / 255.0f, colors[stripe][1] / 255.0f,
                     colors[stripe][2] / 255.0f, 1.0f);
        glClear(GL_COLOR_BUFFER_BIT);
    }
    glDisable(GL_SCISSOR_TEST);
    glFinish();
    require(glGetError() == GL_NO_ERROR, "draw");
}

static void await_capture(Display *display, const char *api) {
    XSync(display, False);
    require(!x_error, "first_swap");
    printf("gl_first_frame api=%s submitted=1\n", api);
    fflush(stdout);
    require(getchar() == 'q', "capture_acknowledgement");
}

static void run_glx(Display *display) {
    const int attributes[] = {
        GLX_X_RENDERABLE, True, GLX_DRAWABLE_TYPE, GLX_WINDOW_BIT,
        GLX_RENDER_TYPE, GLX_RGBA_BIT, GLX_DOUBLEBUFFER, True,
        GLX_RED_SIZE, 8, GLX_GREEN_SIZE, 8, GLX_BLUE_SIZE, 8, None
    };
    int count = 0;
    GLXFBConfig *configs = glXChooseFBConfig(display, DefaultScreen(display), attributes, &count);
    require(configs && count > 0, "glx_configs");
    XVisualInfo *visual = NULL;
    GLXFBConfig selected = NULL;
    for (int index = 0; index < count; ++index) {
        XVisualInfo *candidate = glXGetVisualFromFBConfig(display, configs[index]);
        if (candidate && candidate->depth == 24) {
            visual = candidate;
            selected = configs[index];
            break;
        }
        if (candidate) XFree(candidate);
    }
    require(visual != NULL, "glx_visual");
    require(visual->visualid == XVisualIDFromVisual(DefaultVisual(display, DefaultScreen(display))) &&
            visual->depth == 24, "glx_default_visual_depth24");
    Colormap colormap;
    Window drawable = window(display, visual, &colormap);
    GLXContext context = glXCreateNewContext(display, selected, GLX_RGBA_TYPE, NULL, True);
    require(context && glXIsDirect(display, context), "glx_direct_context");
    require(glXMakeCurrent(display, drawable, context), "glx_current");
    draw();
    glXSwapBuffers(display, drawable);
    await_capture(display, "glx");
    require(glXMakeCurrent(display, None, NULL), "glx_release_current");
    glXDestroyContext(display, context);
    XDestroyWindow(display, drawable);
    XFreeColormap(display, colormap);
    XFree(visual);
    XFree(configs);
}

static void run_egl(Display *display) {
    EGLDisplay egl = eglGetDisplay((EGLNativeDisplayType)display);
    require(egl != EGL_NO_DISPLAY && eglInitialize(egl, NULL, NULL), "egl_initialize");
    require(eglBindAPI(EGL_OPENGL_API), "egl_bind_api");
    const EGLint attributes[] = {
        EGL_SURFACE_TYPE, EGL_WINDOW_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_BIT,
        EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_NONE
    };
    EGLConfig configs[64];
    EGLint count = 0;
    require(eglChooseConfig(egl, attributes, configs, 64, &count) && count > 0, "egl_configs");
    XVisualInfo *visual = NULL;
    EGLConfig selected = NULL;
    for (int index = 0; index < count; ++index) {
        EGLint visual_id;
        require(eglGetConfigAttrib(egl, configs[index], EGL_NATIVE_VISUAL_ID, &visual_id), "egl_visual_id");
        XVisualInfo pattern = {.visualid = (VisualID)visual_id};
        int visual_count;
        XVisualInfo *candidate = XGetVisualInfo(display, VisualIDMask, &pattern, &visual_count);
        if (candidate && visual_count > 0 && candidate->depth == 24) {
            visual = candidate;
            selected = configs[index];
            break;
        }
        if (candidate) XFree(candidate);
    }
    require(visual != NULL, "egl_visual");
    Colormap colormap;
    Window drawable = window(display, visual, &colormap);
    EGLContext context = eglCreateContext(egl, selected, EGL_NO_CONTEXT, NULL);
    EGLSurface surface = eglCreateWindowSurface(egl, selected, (EGLNativeWindowType)drawable, NULL);
    require(context != EGL_NO_CONTEXT && surface != EGL_NO_SURFACE, "egl_context_surface");
    require(eglMakeCurrent(egl, surface, surface, context), "egl_current");
    draw();
    require(eglSwapBuffers(egl, surface), "egl_swap");
    await_capture(display, "egl");
    require(eglMakeCurrent(egl, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT), "egl_release_current");
    require(eglDestroySurface(egl, surface) && eglDestroyContext(egl, context), "egl_destroy");
    require(eglTerminate(egl), "egl_terminate");
    XDestroyWindow(display, drawable);
    XFreeColormap(display, colormap);
    XFree(visual);
}

static void run_cpu(Display *display) {
    XVisualInfo visual;
    require(XMatchVisualInfo(display, DefaultScreen(display), 24, TrueColor, &visual),
            "cpu_visual");
    require(visual.red_mask == 0xff0000 && visual.green_mask == 0xff00 &&
            visual.blue_mask == 0xff, "cpu_color_masks");
    Colormap colormap;
    Window drawable = window(display, &visual, &colormap);
    GC gc = XCreateGC(display, drawable, 0, NULL);
    const unsigned long colors[] = {0x214365, 0xabcdef, 0x1199dd};
    for (int stripe = 0; stripe < 3; ++stripe) {
        XSetForeground(display, gc, colors[stripe]);
        XFillRectangle(display, drawable, gc, stripe * (WIDTH / 3), 0,
                       WIDTH / 3, HEIGHT);
    }
    await_capture(display, "cpu");
    XFreeGC(display, gc);
    XDestroyWindow(display, drawable);
    XFreeColormap(display, colormap);
}

int main(int argc, char **argv) {
    require(argc == 2 && (!strcmp(argv[1], "glx") || !strcmp(argv[1], "egl") ||
                         !strcmp(argv[1], "cpu")), "arguments");
    Display *display = XOpenDisplay(NULL);
    require(display != NULL, "connect");
    XSetErrorHandler(record_error);
    if (!strcmp(argv[1], "cpu")) run_cpu(display);
    else if (!strcmp(argv[1], "egl")) run_egl(display);
    else run_glx(display);
    XSync(display, False);
    require(!x_error, "cleanup");
    XCloseDisplay(display);
    printf("gl_first_frame cleanup=complete\n");
    return 0;
}
