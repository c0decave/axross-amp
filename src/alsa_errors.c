/* alsa-lib's error lines, formatted with their arguments, handed to the
 * program's limiter (src/engine.rs, quiet_alsa_lib). alsa-lib's handler is
 * variadic, and a Rust function cannot take a C variadic's arguments: one
 * of the fixed arguments alone printed the format as it is ("%s write
 * failed") and was, formally, called the wrong way (review 2 of 0.17.1,
 * L-N3). Declared here, not taken from <alsa/asoundlib.h>: no headers are
 * needed to build. */

#include <stdarg.h>
#include <stdio.h>

typedef void (*amp_alsa_said_t)(const char *file, int line, const char *function,
                                int err, const char *text);

typedef void (*amp_snd_lib_error_handler_t)(const char *file, int line,
                                            const char *function, int err,
                                            const char *fmt, ...);
extern int snd_lib_error_set_handler(amp_snd_lib_error_handler_t handler);

static amp_alsa_said_t amp_said;

static void amp_alsa_handler(const char *file, int line, const char *function,
                             int err, const char *fmt, ...)
{
    char text[1024];
    text[0] = '\0';
    if (fmt != NULL) {
        va_list args;
        va_start(args, fmt);
        vsnprintf(text, sizeof text, fmt, args);
        va_end(args);
    }
    if (amp_said != NULL)
        amp_said(file, line, function, err, text);
}

/* alsa-lib's lines to `said` from now on (set once, before any stream). */
int amp_alsa_errors_to(amp_alsa_said_t said)
{
    amp_said = said;
    return snd_lib_error_set_handler(amp_alsa_handler);
}
