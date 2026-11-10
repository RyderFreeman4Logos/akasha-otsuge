#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>
int fsync(int fd) {
 int (*real_fsync)(int)=dlsym(RTLD_NEXT,"fsync");
 const char *root=getenv("R4_ROOT"),*park=getenv("R4_PARK"),*other=getenv("R4_OTHER"),*prefix=getenv("R4_PREFIX");
 char bytes[64]={0};
 if(root&&park&&prefix&&pread(fd,bytes,63,0)>0&&!strncmp(bytes,prefix,strlen(prefix))) {
  if(rename(root,park)||(other?rename(other,root):mkdir(root,0700))) {perror("r4 move");_exit(97);}
  unsetenv("R4_ROOT");fprintf(stderr,"R4_MOVE_OBSERVED\n");
 }
 return real_fsync(fd);
}
