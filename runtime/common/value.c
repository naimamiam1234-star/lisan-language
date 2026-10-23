#include "value.h"
#include <stdio.h>
#include <string.h>

static struct { LisanValueHeader header; unsigned char bytes[1]; } empty = {
    .header = {.refs=SIZE_MAX, .kind=LISAN_KIND_TEXT, .flags=LISAN_FLAG_IMMORTAL},
    .bytes = {0}
};
static LisanText empty_text(void) { return (LisanText){0, empty.bytes}; }
static LisanStatus failure(LisanContext *ctx, LisanStatus status) {
    ctx->error.status = status;
    return status;
}
void lisan_rt_context_init_with_allocator(LisanContext *ctx, LisanAllocator allocator) {
    *ctx = (LisanContext){.allocator=allocator};
}
void lisan_rt_context_init(LisanContext *ctx) {
    lisan_rt_context_init_with_allocator(ctx, lisan_rt_host_allocator());
}
void lisan_rt_context_destroy(LisanContext *ctx) { memset(ctx, 0, sizeof(*ctx)); }
size_t lisan_rt_context_size(void) { return sizeof(LisanContext); }
bool lisan_rt_size_add(size_t a, size_t b, size_t *out) {
    if (b > SIZE_MAX-a) return false;
    *out=a+b; return true;
}
bool lisan_rt_size_mul(size_t a, size_t b, size_t *out) {
    if (a && b > SIZE_MAX/a) return false;
    *out=a*b; return true;
}
bool lisan_rt_size_align_up(size_t n, size_t align, size_t *out) {
    size_t sum;
    if (!align || (align & (align-1)) || !lisan_rt_size_add(n,align-1,&sum)) return false;
    *out=sum & ~(align-1); return true;
}
LisanValueHeader *lisan_rt_text_header(const LisanText *text) {
    return text->data ? (LisanValueHeader *)(void *)text->data-1 : &empty.header;
}
/* Return a scalar's width, rejecting truncated, overlong, surrogate and out of
   Unicode range sequences without reading outside the supplied byte slice. */
static size_t scalar_width(const unsigned char *p, size_t remaining) {
    unsigned char first=p[0]; size_t width; uint32_t scalar, minimum;
    if (first<0x80) return 1;
    if (first>=0xc2 && first<=0xdf) { width=2; scalar=first & 31; minimum=0x80; }
    else if (first>=0xe0 && first<=0xef) { width=3; scalar=first & 15; minimum=0x800; }
    else if (first>=0xf0 && first<=0xf4) { width=4; scalar=first & 7; minimum=0x10000; }
    else return 0;
    if (remaining<width) return 0;
    for (size_t i=1;i<width;i++) {
        if ((p[i]&0xc0)!=0x80) return 0;
        scalar=(scalar<<6)|(p[i]&63);
    }
    return scalar<minimum || scalar>0x10ffff || (scalar>=0xd800 && scalar<=0xdfff) ? 0 : width;
}
static LisanStatus allocate_text(LisanContext *ctx,size_t len,LisanText *out) {
    size_t allocation;
    if (!lisan_rt_size_add(sizeof(LisanValueHeader),len,&allocation)) return failure(ctx,LISAN_SIZE_OVERFLOW);
    if (!len) { *out=empty_text(); return LISAN_OK; }
    LisanValueHeader *header=ctx->allocator.alloc(ctx->allocator.state,allocation,_Alignof(LisanValueHeader));
    if (!header) return failure(ctx,LISAN_OOM);
    *header=(LisanValueHeader){.refs=1,.capacity_bytes=len,.allocation_bytes=allocation,.kind=LISAN_KIND_TEXT};
    *out=(LisanText){len,(const unsigned char *)(header+1)};
    return LISAN_OK;
}
LisanStatus lisan_rt_text_from_utf8(LisanContext *ctx,const unsigned char *bytes,size_t len,LisanText *out) {
    size_t allocation;
    if (!lisan_rt_size_add(sizeof(LisanValueHeader),len,&allocation)) return failure(ctx,LISAN_SIZE_OVERFLOW);
    if (len && !bytes) return failure(ctx,LISAN_INVALID_UTF8);
    for (size_t i=0;i<len;) {
        size_t width=scalar_width(bytes+i,len-i);
        if (!width) return failure(ctx,LISAN_INVALID_UTF8);
        i+=width;
    }
    LisanText result;
    LisanStatus status=allocate_text(ctx,len,&result);
    if (status) return status;
    if (len) memcpy((void *)result.data,bytes,len);
    *out=result; return LISAN_OK;
}
LisanStatus lisan_rt_text_clone(LisanContext *ctx,const LisanText *in,LisanText *out) {
    LisanValueHeader *header=lisan_rt_text_header(in);
    if (!(header->flags & LISAN_FLAG_IMMORTAL)) {
        if (header->refs==SIZE_MAX) return failure(ctx,LISAN_REFCOUNT_OVERFLOW);
        header->refs++;
    }
    *out=in->data ? *in : empty_text(); return LISAN_OK;
}
void lisan_rt_text_release(LisanContext *ctx,LisanText *slot) {
    LisanValueHeader *header=lisan_rt_text_header(slot);
    if (!(header->flags & LISAN_FLAG_IMMORTAL) && --header->refs==0)
        ctx->allocator.dealloc(ctx->allocator.state,header,header->allocation_bytes,_Alignof(LisanValueHeader));
    *slot=empty_text();
}
LisanStatus lisan_rt_text_concat(LisanContext *ctx,const LisanText *a,const LisanText *b,LisanText *out) {
    size_t len;
    if (!lisan_rt_size_add(a->byte_len,b->byte_len,&len)) return failure(ctx,LISAN_SIZE_OVERFLOW);
    LisanText result;
    LisanStatus status=allocate_text(ctx,len,&result);
    if (status) return status;
    if (a->byte_len) memcpy((void *)result.data,a->data,a->byte_len);
    if (b->byte_len) memcpy((unsigned char *)(void *)result.data+a->byte_len,b->data,b->byte_len);
    *out=result; return LISAN_OK;
}
LisanStatus lisan_rt_text_at(LisanContext *ctx,const LisanText *text,size_t index,LisanText *out) {
    for (size_t offset=0;offset<text->byte_len;) {
        size_t width=scalar_width(text->data+offset,text->byte_len-offset);
        if (!index) return lisan_rt_text_from_utf8(ctx,text->data+offset,width,out);
        index--; offset+=width;
    }
    return failure(ctx,LISAN_BOUNDS);
}
size_t lisan_rt_text_scalar_len(const LisanText *text) {
    size_t count=0;
    for (size_t i=0;i<text->byte_len;i++) if ((text->data[i]&0xc0)!=0x80) count++;
    return count;
}
bool lisan_rt_text_equal(const LisanText *a,const LisanText *b) {
    return a->byte_len==b->byte_len && (!a->byte_len || memcmp(a->data,b->data,a->byte_len)==0);
}
void lisan_rt_text_print(const LisanText *text) {
    if (text->byte_len) (void)fwrite(text->data,1,text->byte_len,stdout);
}

/* Lists deliberately contain only scalar/text elements in this increment.
   One aligned allocation owns its header and contiguous elements. Mutation
   stages a fresh allocation so every allocation/clone failure is atomic. */
struct LisanListStorage {
    size_t refs, len, allocation_bytes;
    const LisanTypeOps *ops;
    max_align_t alignment;
    unsigned char data[];
};
_Static_assert(offsetof(struct LisanListStorage, data) % _Alignof(max_align_t)==0,
               "list element alignment");
#define SCALAR_OPS(name, T) \
static LisanStatus clone_##name(LisanContext *ctx,const void *in,void *out) { \
    (void)ctx; \
    memcpy(out,in,sizeof(T)); \
    return LISAN_OK; \
} \
static void release_##name(LisanContext *ctx,void *value) { \
    (void)ctx; (void)value; \
} \
static bool equal_##name(const void *a,const void *b) { \
    T x,y; \
    memcpy(&x,a,sizeof(T)); \
    memcpy(&y,b,sizeof(T)); \
    return x==y; \
} \
const LisanTypeOps lisan_rt_ops_##name={ \
    sizeof(T),_Alignof(T),clone_##name,release_##name,equal_##name \
};
SCALAR_OPS(i8,int8_t)
SCALAR_OPS(i16,int16_t)
SCALAR_OPS(i32,int32_t)
SCALAR_OPS(i64,int64_t)
SCALAR_OPS(u8,uint8_t)
SCALAR_OPS(u16,uint16_t)
SCALAR_OPS(u32,uint32_t)
SCALAR_OPS(u64,uint64_t)
SCALAR_OPS(f32,float)
SCALAR_OPS(f64,double)
SCALAR_OPS(bool,bool)
static LisanStatus clone_text_value(LisanContext *ctx,const void *in,void *out) {
    return lisan_rt_text_clone(ctx,in,out);
}
static void release_text_value(LisanContext *ctx,void *v) {
    lisan_rt_text_release(ctx,v);
}
static bool equal_text_value(const void *a,const void *b) {
    return lisan_rt_text_equal(a,b);
}
const LisanTypeOps lisan_rt_ops_text={
    sizeof(LisanText),_Alignof(LisanText),
    clone_text_value,release_text_value,equal_text_value
};
void lisan_rt_list_release(LisanContext *ctx,LisanList *slot) {
    LisanList list=*slot;
    *slot=NULL;
    if (!list || --list->refs) return;
    for (size_t i=list->len;i>0;i--) {
        list->ops->release(ctx,list->data+(i-1)*list->ops->size);
    }
    ctx->allocator.dealloc(ctx->allocator.state,list,list->allocation_bytes,
                          _Alignof(struct LisanListStorage));
}
LisanStatus lisan_rt_list_clone(LisanContext *ctx,const LisanList *in,LisanList *out) {
    if (*in && (*in)->refs==SIZE_MAX) return failure(ctx,LISAN_REFCOUNT_OVERFLOW);
    if (*in) (*in)->refs++;
    *out=*in;
    return LISAN_OK;
}
static bool list_ops_supported(const LisanTypeOps *ops) {
    return ops==&lisan_rt_ops_i8 || ops==&lisan_rt_ops_i16 ||
        ops==&lisan_rt_ops_i32 || ops==&lisan_rt_ops_i64 ||
        ops==&lisan_rt_ops_u8 || ops==&lisan_rt_ops_u16 ||
        ops==&lisan_rt_ops_u32 || ops==&lisan_rt_ops_u64 ||
        ops==&lisan_rt_ops_f32 || ops==&lisan_rt_ops_f64 ||
        ops==&lisan_rt_ops_bool || ops==&lisan_rt_ops_text;
}
static LisanStatus allocate_list(LisanContext *ctx,const LisanTypeOps *ops,
                                 size_t len,LisanList *out) {
    size_t bytes,total;
    if (!list_ops_supported(ops) || !ops->size || !ops->align ||
        ops->align>_Alignof(struct LisanListStorage) || ops->size%ops->align ||
        !lisan_rt_size_mul(len,ops->size,&bytes) ||
        !lisan_rt_size_add(offsetof(struct LisanListStorage,data),bytes,&total))
        return failure(ctx,LISAN_SIZE_OVERFLOW);
    LisanList list=ctx->allocator.alloc(ctx->allocator.state,total,
                                       _Alignof(struct LisanListStorage));
    if (!list) return failure(ctx,LISAN_OOM);
    list->refs=1;
    list->len=0;
    list->allocation_bytes=total;
    list->ops=ops;
    *out=list;
    return LISAN_OK;
}
LisanStatus lisan_rt_list_new(LisanContext *ctx,const LisanTypeOps *ops,
                              size_t len,const void *const *items,LisanList *out) {
    LisanList staged;
    LisanStatus status=allocate_list(ctx,ops,len,&staged);
    if (status) return status;
    for (size_t i=0;i<len;i++) {
        status=ops->clone(ctx,items[i],staged->data+i*ops->size);
        if (status) {
            lisan_rt_list_release(ctx,&staged);
            return status;
        }
        staged->len++;
    }
    *out=staged;
    return LISAN_OK;
}
size_t lisan_rt_list_len(const LisanList *slot) {
    return *slot ? (*slot)->len : 0;
}
LisanStatus lisan_rt_list_at(LisanContext *ctx,const LisanList *slot,
                             size_t index,void *out) {
    LisanList list=*slot;
    if (!list || index>=list->len) return failure(ctx,LISAN_BOUNDS);
    return list->ops->clone(ctx,list->data+index*list->ops->size,out);
}
static LisanStatus list_change(LisanContext *ctx,const LisanList *slot,
                               size_t index,const void *value,bool append,
                               LisanList *out) {
    LisanList list=*slot,staged;
    if (!list || (!append && index>=list->len)) return failure(ctx,LISAN_BOUNDS);
    size_t len=list->len;
    if (append && !lisan_rt_size_add(len,1,&len)) return failure(ctx,LISAN_SIZE_OVERFLOW);
    LisanStatus status=allocate_list(ctx,list->ops,len,&staged);
    if (status) return status;
    for (size_t i=0;i<len;i++) {
        const void *source=(i==index) ? value : list->data+i*list->ops->size;
        status=list->ops->clone(ctx,source,staged->data+i*list->ops->size);
        if (status) {
            lisan_rt_list_release(ctx,&staged);
            return status;
        }
        staged->len++;
    }
    *out=staged;
    return LISAN_OK;
}
LisanStatus lisan_rt_list_set(LisanContext *ctx,const LisanList *slot,
                              size_t index,const void *value,LisanList *out) {
    return list_change(ctx,slot,index,value,false,out);
}
LisanStatus lisan_rt_list_append(LisanContext *ctx,const LisanList *slot,
                                 const void *value,LisanList *out) {
    return list_change(ctx,slot,lisan_rt_list_len(slot),value,true,out);
}
bool lisan_rt_list_equal(const LisanList *a,const LisanList *b) {
    if (!*a || !*b) return *a==*b;
    if ((*a)->ops!=(*b)->ops || (*a)->len!=(*b)->len) return false;
    for (size_t i=0;i<(*a)->len;i++) {
        if (!(*a)->ops->equal((*a)->data+i*(*a)->ops->size,
                             (*b)->data+i*(*b)->ops->size)) return false;
    }
    return true;
}
