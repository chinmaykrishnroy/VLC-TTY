/* ABI-level test shim. It is NOT a video decoder and MUST NEVER ship as LibVLC. */
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef void *(*lock_cb_t)(void *, void **);
typedef void (*display_cb_t)(void *, void *);
typedef unsigned (*format_cb_t)(void **, char *, unsigned *, unsigned *, unsigned *, unsigned *);
typedef struct track { int id; char *name; struct track *next; } track_t;
typedef struct { int state, volume, mute; int64_t pos, length; float rate;
    lock_cb_t lock; display_cb_t display; format_cb_t format;
    int w,h; unsigned pitch; char chroma[4]; } player_t;
const char *libvlc_get_version(void) { return "3.0.99 fake-test-shim"; }
void *libvlc_new(int n, const char *const *args) { (void)n;(void)args;return malloc(1); }
void libvlc_release(void *v) { free(v); }
void *libvlc_media_player_new(void *v) { (void)v; player_t *p=calloc(1,sizeof(player_t));p->volume=75;p->length=120000;p->rate=1; return p; }
void libvlc_media_player_release(void *v) { free(v); }
void *libvlc_media_new_path(void *v, const char *s) { (void)v;return strdup(s); }
void *libvlc_media_new_location(void *v, const char *s) { (void)v;return strdup(s); }
void libvlc_media_release(void *v) { free(v); }
void libvlc_media_player_set_media(void *v, void *m) { (void)v;(void)m; }
void libvlc_video_set_callbacks(void *v, lock_cb_t l, void *u, display_cb_t d,void *opaque) { (void)u;(void)opaque;((player_t*)v)->lock=l;((player_t*)v)->display=d; }
void libvlc_video_set_format_callbacks(void *v,format_cb_t f,void *c) { (void)c;((player_t*)v)->format=f; }
int libvlc_media_player_play(void *v) { player_t*p=v;p->state=3;
    unsigned w=1920,h=1080,pitch=0,lines=0; void *opaque=0;
    if (!p->format || !p->format(&opaque,p->chroma,&w,&h,&pitch,&lines)) return -1;
    p->w=(int)w;p->h=(int)h;p->pitch=pitch;
    void *planes[1]={NULL};void *picture=p->lock(NULL,planes);
    if (planes[0]) { unsigned char *data=planes[0];for (unsigned y=0;y<h;y++)for(unsigned x=0;x<w;x++) {
        int o=y*pitch+x*4;data[o]=0;data[o+1]=(unsigned char)((y*255)/h);data[o+2]=(unsigned char)((x*255)/w);data[o+3]=255;
    }}
    p->display(NULL,picture);
    return 0;
}
void libvlc_media_player_set_pause(void *v,int pause) { ((player_t*)v)->state=pause?4:3; }
void libvlc_media_player_stop(void *v){((player_t*)v)->state=5;}
int64_t libvlc_media_player_get_time(void *v){return ((player_t*)v)->pos;}
int64_t libvlc_media_player_get_length(void *v){return ((player_t*)v)->length;}
void libvlc_media_player_set_time(void *v,int64_t t){((player_t*)v)->pos=t;}
int libvlc_media_player_get_state(void *v){return ((player_t*)v)->state;}
int libvlc_audio_get_volume(void *v){return ((player_t*)v)->volume;}
int libvlc_audio_set_volume(void *v,int vol){((player_t*)v)->volume=vol;return 0;}
int libvlc_audio_get_mute(void *v){return ((player_t*)v)->mute;}
void libvlc_audio_set_mute(void *v,int mute){((player_t*)v)->mute=mute;}
float libvlc_media_player_get_rate(void *v){return ((player_t*)v)->rate;}
int libvlc_media_player_set_rate(void *v,float rate){((player_t*)v)->rate=rate;return 0;}
int libvlc_video_get_spu(void *v){(void)v;return -1;}
int libvlc_video_set_spu(void *v,int id){(void)v;(void)id;return 0;}
track_t *libvlc_video_get_spu_description(void *v){(void)v;return NULL;}
int libvlc_audio_get_track(void *v){(void)v;return 0;}
int libvlc_audio_set_track(void *v,int id){(void)v;(void)id;return 0;}
track_t *libvlc_audio_get_track_description(void *v){(void)v;return NULL;}
void libvlc_track_description_list_release(track_t *t){(void)t;}
