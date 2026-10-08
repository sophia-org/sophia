/* Private implementation of the development-seat launcher's final boundary. */
#ifndef SOPHIA_DEVELOPMENT_RESTRICT_H
#define SOPHIA_DEVELOPMENT_RESTRICT_H
#include <sys/types.h>
int development_scope(void);
int development_socket_filter(void);
int development_drop(uid_t uid, gid_t gid, pid_t parent);
int development_verify_drop(uid_t uid, gid_t gid);
#endif
