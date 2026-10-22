#ifndef LISAN_VALUE_H
#define LISAN_VALUE_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef int32_t LisanStatus;
enum { LISAN_OK=0, LISAN_OOM=1, LISAN_SIZE_OVERFLOW=2, LISAN_BOUNDS=3,
       LISAN_MISSING_KEY=4, LISAN_INVALID_UTF8=5, LISAN_REFCOUNT_OVERFLOW=6 };
typedef struct { void *state; void *(*alloc)(void *,size_t,size_t);
    void (*dealloc)(void *,void *,size_t,size_t); } LisanAllocator;
typedef struct { LisanStatus status; size_t source_site; } LisanError;
typedef struct { LisanAllocator allocator; LisanError error; } LisanContext;
typedef struct { size_t byte_len; const unsigned char *data; } LisanText;
enum { LISAN_KIND_TEXT=1, LISAN_FLAG_IMMORTAL=1 };
/* The union rounds header size and alignment to max_align_t on each target.
   Literal emitters must use this exact target layout and immortal flag. */
typedef union LisanValueHeader {
    struct { size_t refs, capacity_bytes, allocation_bytes; uint32_t kind, flags; };
    max_align_t alignment;
} LisanValueHeader;
_Static_assert(sizeof(LisanValueHeader) % _Alignof(max_align_t)==0,"header alignment");
_Static_assert(offsetof(LisanText,data)==sizeof(size_t),"text ABI");
typedef struct { size_t size, align;
    LisanStatus (*clone)(LisanContext *,const void *,void *);
    void (*release)(LisanContext *,void *);
    bool (*equal)(const void *,const void *); } LisanTypeOps;
LisanAllocator lisan_rt_host_allocator(void);
void lisan_rt_context_init(LisanContext *);
void lisan_rt_context_init_with_allocator(LisanContext *,LisanAllocator);
void lisan_rt_context_destroy(LisanContext *);
size_t lisan_rt_context_size(void);
bool lisan_rt_size_add(size_t,size_t,size_t *);
bool lisan_rt_size_mul(size_t,size_t,size_t *);
bool lisan_rt_size_align_up(size_t,size_t,size_t *);
/* Inputs must be valid runtime-owned descriptors (or zero empty descriptors),
   allocated by this context's allocator. Outputs are distinct empty slots on
   success; failures leave any output bytes unchanged. No input/output aliasing.
   Borrowed byte buffers enter only through from_utf8, which copies them. The
   caller releases all owned values before destroying the context. */
LisanValueHeader *lisan_rt_text_header(const LisanText *);
LisanStatus lisan_rt_text_from_utf8(LisanContext *,const unsigned char *,size_t,LisanText *);
LisanStatus lisan_rt_text_clone(LisanContext *,const LisanText *,LisanText *);
LisanStatus lisan_rt_text_concat(LisanContext *,const LisanText *,const LisanText *,LisanText *);
LisanStatus lisan_rt_text_at(LisanContext *,const LisanText *,size_t,LisanText *);
void lisan_rt_text_release(LisanContext *,LisanText *);
size_t lisan_rt_text_scalar_len(const LisanText *);
bool lisan_rt_text_equal(const LisanText *,const LisanText *);
void lisan_rt_text_print(const LisanText *);
#endif
