#include "value.h"
#include <assert.h>
#include <stdlib.h>
#include <string.h>

typedef struct { size_t attempts, live_blocks, live_bytes, fail_at; } State;
static void *allocate(void *p, size_t n, size_t alignment) {
    State *s = p;
    assert(alignment <= _Alignof(max_align_t));
    if (++s->attempts == s->fail_at) return NULL;
    void *v = malloc(n);
    if (v) { s->live_blocks++; s->live_bytes += n; }
    return v;
}
static void deallocate(void *p, void *v, size_t n, size_t alignment) {
    State *s = p; (void)alignment;
    assert(s->live_blocks && s->live_bytes >= n);
    s->live_blocks--; s->live_bytes -= n; free(v);
}
int main(void) {
    State s = {0}; LisanContext c;
    lisan_rt_context_init_with_allocator(&c, (LisanAllocator){&s, allocate, deallocate});
    assert(lisan_rt_context_size() == sizeof(c));
    size_t n = 91;
    assert(!lisan_rt_size_add(SIZE_MAX, 1, &n) && n == 91);
    assert(!lisan_rt_size_mul(SIZE_MAX, 2, &n) && n == 91);
    assert(!lisan_rt_size_align_up(SIZE_MAX, 16, &n) && n == 91);
    assert(lisan_rt_size_align_up(17, 16, &n) && n == 32);
    LisanAllocator host=lisan_rt_host_allocator();
    for(size_t align=1;align<=256;align*=2) {
        void *block=host.alloc(host.state,37,align);
        assert(block && (uintptr_t)block % align == 0);
        host.dealloc(host.state,block,37,align);
    }
    assert(!host.alloc(host.state,SIZE_MAX,16));
    LisanText a = {0}, b = {0}, out = {123, (const unsigned char *)1};
    LisanText oversized={SIZE_MAX,(const unsigned char *)1}, one={1,(const unsigned char *)1};
    assert(lisan_rt_text_concat(&c,&oversized,&one,&out)==LISAN_SIZE_OVERFLOW);
    assert(out.byte_len==123 && s.attempts==0);
    s.fail_at = s.attempts + 1;
    assert(lisan_rt_text_from_utf8(&c, (const unsigned char *)"abc", 3, &out) == LISAN_OOM);
    assert(out.byte_len == 123 && out.data == (const unsigned char *)1);
    s.fail_at = 0;
    assert(lisan_rt_text_from_utf8(&c, (const unsigned char *)"", SIZE_MAX, &out) == LISAN_SIZE_OVERFLOW);
    const unsigned char *bad[] = {(const unsigned char *)"\xc0\x80", (const unsigned char *)"\xed\xa0\x80", (const unsigned char *)"\xf4\x90\x80\x80", (const unsigned char *)"\x80", (const unsigned char *)"\xe2\x82"};
    size_t lens[] = {2,3,4,1,2};
    for (size_t i=0;i<5;i++) assert(lisan_rt_text_from_utf8(&c,bad[i],lens[i],&out)==LISAN_INVALID_UTF8);
    assert(out.byte_len == 123);
    assert(lisan_rt_text_from_utf8(&c, (const unsigned char *)"سَ😀", 8, &a) == LISAN_OK);
    assert(lisan_rt_text_scalar_len(&a) == 3);
    assert((uintptr_t)lisan_rt_text_header(&a) % _Alignof(max_align_t) == 0);
    assert(lisan_rt_text_clone(&c,&a,&b)==LISAN_OK);
    LisanValueHeader *h = lisan_rt_text_header(&a);
    size_t refs = h->refs; h->refs = SIZE_MAX;
    assert(lisan_rt_text_clone(&c,&a,&out)==LISAN_REFCOUNT_OVERFLOW && out.byte_len==123);
    h->refs = refs;
    assert(lisan_rt_text_at(&c,&a,3,&out)==LISAN_BOUNDS && out.byte_len==123);
    for (size_t fail=1;fail<=3;fail++) {
        LisanText x={0},y={0},z={0}; s.fail_at=s.attempts+fail;
        LisanStatus status=lisan_rt_text_from_utf8(&c,(const unsigned char *)"ab",2,&x);
        if (!status) status=lisan_rt_text_concat(&c,&x,&a,&y);
        if (!status) status=lisan_rt_text_at(&c,&y,0,&z);
        assert(status==LISAN_OOM);
        size_t attempts=s.attempts;
        lisan_rt_text_release(&c,&z);lisan_rt_text_release(&c,&y);lisan_rt_text_release(&c,&x);
        assert(s.attempts==attempts && s.live_blocks==1);
    }
    s.fail_at=0;
    for(size_t i=0;i<100000;i++) {
        LisanText x={0},y={0};
        assert(lisan_rt_text_concat(&c,&a,&b,&x)==LISAN_OK);
        assert(lisan_rt_text_at(&c,&x,2,&y)==LISAN_OK);
        assert(y.byte_len==4);
        lisan_rt_text_release(&c,&y);lisan_rt_text_release(&c,&x);
        assert(s.live_blocks==1);
    }
    LisanText empty={0};
    assert(lisan_rt_text_from_utf8(&c,NULL,0,&empty)==LISAN_OK);
    assert(lisan_rt_text_header(&empty)->flags & LISAN_FLAG_IMMORTAL);
    lisan_rt_text_release(&c,&empty);lisan_rt_text_release(&c,&empty);
    lisan_rt_text_release(&c,&b);lisan_rt_text_release(&c,&a);
    assert(s.live_blocks==0 && s.live_bytes==0);
    LisanText nul={0},copy={0};
    assert(lisan_rt_text_from_utf8(&c,(const unsigned char *)"A\0B",3,&nul)==LISAN_OK);
    assert(lisan_rt_text_clone(&c,&nul,&copy)==LISAN_OK && lisan_rt_text_equal(&nul,&copy));
    assert(lisan_rt_text_scalar_len(&nul)==3);lisan_rt_text_print(&nul);
    lisan_rt_text_release(&c,&copy);lisan_rt_text_release(&c,&nul);
    assert(!s.live_blocks);lisan_rt_context_destroy(&c);
    lisan_rt_context_init(&c);
    assert(lisan_rt_text_from_utf8(&c,(const unsigned char *)"host",4,&a)==LISAN_OK);
    lisan_rt_text_release(&c,&a);lisan_rt_context_destroy(&c);
}
