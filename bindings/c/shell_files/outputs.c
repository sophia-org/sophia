/* KDL field transcription; cross-field validation is in validation.c. */
#include "internal.h"
int sf_check_content_output_facts_entry(const struct sophia_sf_content_output_facts_entry *v)
{
    if (!v->output_id || !v->output_generation || !v->local_width || !v->local_height ||
        v->scale_numerator < 1 || v->scale_numerator > 32 || v->scale_denominator < 1 ||
        v->scale_denominator > 4 || !v->scale_generation)
        return -1;
    return 0;
}
void sf_put_content_output_facts_entry(uint8_t *b,
                                       const struct sophia_sf_content_output_facts_entry *v)
{
    memset(b, 0, 40);
    sf_put(b + 0, (uint64_t)v->output_id, 8);
    sf_put(b + 8, (uint64_t)v->output_generation, 8);
    sf_put(b + 16, (uint64_t)v->local_width, 4);
    sf_put(b + 20, (uint64_t)v->local_height, 4);
    sf_put(b + 24, (uint64_t)v->scale_numerator, 4);
    sf_put(b + 28, (uint64_t)v->scale_denominator, 4);
    sf_put(b + 32, (uint64_t)v->scale_generation, 8);
}
int sf_take_content_output_facts_entry(const uint8_t *b,
                                       struct sophia_sf_content_output_facts_entry *v)
{
    v->output_id = (uint64_t)sf_get(b + 0, 8);
    v->output_generation = (uint64_t)sf_get(b + 8, 8);
    v->local_width = (uint32_t)sf_get(b + 16, 4);
    v->local_height = (uint32_t)sf_get(b + 20, 4);
    v->scale_numerator = (uint32_t)sf_get(b + 24, 4);
    v->scale_denominator = (uint32_t)sf_get(b + 28, 4);
    v->scale_generation = (uint64_t)sf_get(b + 32, 8);
    return sf_check_content_output_facts_entry(v);
}
int sf_check_outputs(const struct sophia_sf_outputs *v)
{
    if (!v->transaction || !v->grant_connection_epoch || !v->grant_content_epoch ||
        !v->facts_generation || v->output_count > 16)
        return -1;
    return 0;
}
void sf_put_outputs(uint8_t *b, const struct sophia_sf_outputs *v)
{
    memset(b, 0, 40);
    sf_put(b + 0, (uint64_t)v->transaction, 8);
    sf_put(b + 8, (uint64_t)v->grant_connection_epoch, 8);
    sf_put(b + 16, (uint64_t)v->grant_content_epoch, 8);
    sf_put(b + 24, (uint64_t)v->facts_generation, 8);
    sf_put(b + 32, (uint64_t)v->output_count, 4);
}
int sf_take_outputs(const uint8_t *b, struct sophia_sf_outputs *v)
{
    v->transaction = (uint64_t)sf_get(b + 0, 8);
    v->grant_connection_epoch = (uint64_t)sf_get(b + 8, 8);
    v->grant_content_epoch = (uint64_t)sf_get(b + 16, 8);
    v->facts_generation = (uint64_t)sf_get(b + 24, 8);
    v->output_count = (uint32_t)sf_get(b + 32, 4);
    if (!sf_zero(b + 36, 4))
        return -1;
    return sf_check_outputs(v);
}
