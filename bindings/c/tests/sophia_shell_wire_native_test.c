#include "../sophia_shell_native_launcher.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static unsigned nibble(char c)
{
    if (c>='0' && c<='9') return (unsigned)(c-'0');
    if (c>='a' && c<='f') return (unsigned)(c-'a')+10;
    abort();
}

int main(int argc, char **argv)
{
    assert(argc==2);
    FILE *file=fopen(argv[1],"r"); assert(file);
    char line[4096]; uint8_t bytes[2048]; unsigned count=0;
    static const size_t lengths[]={56,84,110,184,104,108,144,124,124,128,28};
    while (fgets(line,sizeof(line),file)) {
        char *hex=strchr(line,' '); assert(hex); *hex++=0;
        size_t size=strcspn(hex,"\r\n"); assert(size%2==0 && size/2<=sizeof(bytes));
        for (size_t i=0; i<size/2; ++i) bytes[i]=(uint8_t)(16*nibble(hex[2*i])+nibble(hex[2*i+1]));
        struct sophia_shell_frame frame;
        int envelope=sophia_shell_frame_decode(bytes,size/2,&frame);
        int valid=envelope==SOPHIA_SHELL_OK && sophia_shell_native_launcher_validate(&frame)==SOPHIA_SHELL_OK;
        int expected=strncmp(line,"reject-",7)!=0;
        if (valid!=expected) { fprintf(stderr,"classification mismatch: %s\n",line); abort(); }
        if (!strncmp(line,"native-launcher-",16)) {
            assert(frame.kind==187+count && frame.payload_bytes==lengths[count]);
            assert(frame.transaction==25);
            /* Exact sent fixture identities, read independently without Rust. */
            assert(frame.payload[0]==2 && frame.payload[8]==3);
            for (size_t n=0; n<frame.payload_bytes; ++n) {
                struct sophia_shell_frame cut=frame; cut.payload_bytes=n;
                assert(sophia_shell_native_launcher_validate(&cut)!=SOPHIA_SHELL_OK);
            }
            uint8_t saved=bytes[24]; bytes[24]=0;
            assert(sophia_shell_native_launcher_validate(&frame)==SOPHIA_SHELL_INVALID);
            bytes[24]=saved;
            if (frame.kind==193) {
                assert(!memcmp(frame.payload+132,"caf\xc3\xa9 \xe6\x9d\xb1\xe4\xba\xac",12));
                bytes[24+132]=0xff;
                assert(sophia_shell_native_launcher_validate(&frame)==SOPHIA_SHELL_INVALID);
            }
        }
        ++count;
    }
    assert(!ferror(file) && count); fclose(file);
    printf("sophia_shell_native_launcher records=%u payload=pass lifecycle=not_run\n",count);
    return 0;
}
