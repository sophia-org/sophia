(set-option :produce-models true)
(set-logic QF_NIA)
(include "validation/architecture/generated/sophia-wm-file-rows-facts.smt2")

(define-fun u32_max () Int 4294967295)
(define-fun u64_max () Int 18446744073709551615)

(echo "secure_row_counts_are_field_representable")
(push)
(assert (not (and
  (> max_record_count 0)
  (<= max_record_count u32_max)
  (<= wm_v1_max_outputs u32_max)
  (<= wm_v1_max_bindings u32_max)
  (<= wm_v1_max_surfaces u32_max))))
(check-sat)
(pop)

; The maxima range over both ordinary and capability-gated file rows.
(echo "secure_record_products_fit_u64")
(push)
(assert (> (* max_record_count max_record_width) u64_max))
(check-sat)
(pop)

(echo "negative_unchecked_u32_record_product")
(push)
(assert (> (* u32_max snapshot_surface_record_width) u32_max))
(check-sat)
(pop)
