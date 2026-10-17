#ifndef SOPHIA_REQUESTER_SESSION_INTERNAL_H
#define SOPHIA_REQUESTER_SESSION_INTERNAL_H
#include "../sophia_portal_requester_session.h"
enum {
  PQR_UPLOAD_BEGIN = 1,
  PQR_UPLOAD_WRITING,
  PQR_UPLOAD_ENDING,
  PQR_UPLOAD_SETTLING
};
int pqr_fail(struct sophia_pqr *, int);
int pqr_advance(struct sophia_pqr *, uint64_t);
int pqr_drive(struct sophia_pqr *);
size_t pqr_transfer_slot(const struct sophia_pqr *,
                         struct sophia_pqr_request_id);
int pqr_drive_upload(struct sophia_pqr *);
#endif
