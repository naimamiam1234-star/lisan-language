#include "value.h"
#include <stdlib.h>
/* Overallocate for alignment and store the original malloc pointer immediately
   before the aligned block. This also supports alignments above max_align_t. */
static void *host_alloc(void *state,size_t size,size_t alignment) {
    (void)state;
    if (alignment< _Alignof(void *)) alignment=_Alignof(void *);
    size_t total;
    if ((alignment & (alignment-1)) ||
        !lisan_rt_size_add(size,alignment-1,&total) ||
        !lisan_rt_size_add(total,sizeof(void *),&total)) return NULL;
    void *base=malloc(total);
    if (!base) return NULL;
    uintptr_t address=(uintptr_t)base+sizeof(void *);
    void *aligned=(void *)((address+alignment-1)&~(uintptr_t)(alignment-1));
    ((void **)aligned)[-1]=base;
    return aligned;
}
static void host_dealloc(void *state,void *ptr,size_t size,size_t alignment) {
    (void)state; (void)size; (void)alignment;
    if (ptr) free(((void **)ptr)[-1]);
}
LisanAllocator lisan_rt_host_allocator(void) {
    return (LisanAllocator){NULL,host_alloc,host_dealloc};
}
