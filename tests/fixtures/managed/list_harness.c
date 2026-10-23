/* White-box arithmetic/refcount checks intentionally share the implementation. */
#include "../../../runtime/common/value.c"
#include <assert.h>
#include <stdlib.h>

typedef struct { size_t attempts,live,fail_at; } TestAllocator;
static void *test_alloc(void *state,size_t bytes,size_t alignment) {
    TestAllocator *test=state;
    assert(alignment<=_Alignof(max_align_t));
    test->attempts++;
    if (test->attempts==test->fail_at) return NULL;
    void *p=malloc(bytes);
    if (p) test->live++;
    return p;
}
static void test_free(void *state,void *p,size_t bytes,size_t alignment) {
    TestAllocator *test=state;
    (void)bytes; (void)alignment;
    if (p) { assert(test->live); test->live--; free(p); }
}
static void unchanged_output(LisanList result) { assert(result==(LisanList)(uintptr_t)17); }
int main(void) {
    TestAllocator test={0};
    LisanContext ctx;
    lisan_rt_context_init_with_allocator(&ctx,(LisanAllocator){&test,test_alloc,test_free});
    int32_t a=1,b=2,replacement=9,result=0;
    const void *items[]={&a,&b};
    LisanList list=NULL,alias=NULL,out=(LisanList)(uintptr_t)17;
    assert(lisan_rt_list_new(&ctx,&lisan_rt_ops_i32,2,items,&list)==LISAN_OK);
    assert(lisan_rt_list_clone(&ctx,&list,&alias)==LISAN_OK);
    test.fail_at=test.attempts+1;
    assert(lisan_rt_list_set(&ctx,&alias,0,&replacement,&out)==LISAN_OOM);
    unchanged_output(out);
    assert(test.live==1);
    assert(lisan_rt_list_at(&ctx,&alias,0,&result)==LISAN_OK && result==1);
    assert(lisan_rt_list_at(&ctx,&list,0,&result)==LISAN_OK && result==1);
    test.fail_at=0;
    LisanList changed=NULL;
    assert(lisan_rt_list_set(&ctx,&alias,0,&replacement,&changed)==LISAN_OK);
    assert(lisan_rt_list_at(&ctx,&changed,0,&result)==LISAN_OK && result==9);
    assert(!lisan_rt_list_equal(&list,&changed));
    assert(lisan_rt_list_set(&ctx,&list,2,&replacement,&out)==LISAN_BOUNDS);
    unchanged_output(out);
    assert(lisan_rt_list_at(&ctx,&list,2,&result)==LISAN_BOUNDS && result==9);
    size_t attempts=test.attempts;
    assert(lisan_rt_list_new(&ctx,&lisan_rt_ops_i32,SIZE_MAX,items,&out)==LISAN_SIZE_OVERFLOW);
    assert(test.attempts==attempts); unchanged_output(out);
    list->refs=SIZE_MAX;
    assert(lisan_rt_list_clone(&ctx,&list,&out)==LISAN_REFCOUNT_OVERFLOW);
    unchanged_output(out); assert(list->refs==SIZE_MAX);
    list->refs=2;
    size_t len=list->len; list->len=SIZE_MAX;
    assert(lisan_rt_list_append(&ctx,&list,&replacement,&out)==LISAN_SIZE_OVERFLOW);
    unchanged_output(out); assert(test.attempts==attempts);
    list->len=len;
    lisan_rt_list_release(&ctx,&changed);
    lisan_rt_list_release(&ctx,&alias);
    lisan_rt_list_release(&ctx,&list);
    assert(!test.live && test.attempts==attempts);
    lisan_rt_list_release(&ctx,&list);
    assert(test.attempts==attempts);

    LisanText text={0};
    assert(lisan_rt_text_from_utf8(&ctx,(const unsigned char *)"x",1,&text)==LISAN_OK);
    const void *texts[]={&text,&text};
    LisanValueHeader *header=lisan_rt_text_header(&text);
    header->refs=SIZE_MAX-1;
    assert(lisan_rt_list_new(&ctx,&lisan_rt_ops_text,2,texts,&out)==LISAN_REFCOUNT_OVERFLOW);
    unchanged_output(out); assert(header->refs==SIZE_MAX-1 && test.live==1);
    header->refs=1;
    assert(lisan_rt_list_new(&ctx,&lisan_rt_ops_text,2,texts,&list)==LISAN_OK);
    /* Fail after cloning an earlier element during a staged mutation. */
    header->refs=SIZE_MAX-1;
    assert(lisan_rt_list_set(&ctx,&list,1,&text,&out)==LISAN_REFCOUNT_OVERFLOW);
    unchanged_output(out); assert(header->refs==SIZE_MAX-1 && test.live==2);
    header->refs=3;
    lisan_rt_list_release(&ctx,&list);
    lisan_rt_text_release(&ctx,&text);
    assert(!test.live);
    LisanTypeOps invalid=lisan_rt_ops_i32;
    assert(lisan_rt_list_new(&ctx,&invalid,0,NULL,&out)==LISAN_SIZE_OVERFLOW);
    unchanged_output(out); assert(!test.live);
    lisan_rt_context_destroy(&ctx);
    return 0;
}
