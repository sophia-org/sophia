impl LiveProductionNativeScanout {
        pub fn selection(&self, index: usize) -> crate::LibdrmNativePrimaryPlaneSelection {
            self.heads[index].selection
        }

        pub fn card(&self, index: usize) -> &crate::RealAtomicScanoutCard {
            self.groups[self.heads[index].group].session.card()
        }

        /// One head and its output's exporter together.
        ///
        /// They live in different tables now, and most work touches both. Handing
        /// out the pair from one place keeps every caller from having to spell out
        /// the disjoint borrow itself.
        fn head_and_exporter(
            &mut self,
            index: usize,
            output: OutputId,
        ) -> (
            &mut LiveProductionNativeHead,
            &mut crate::NativeGbmRenderedScanoutBufferDiscoveryExporter<
                crate::RealAtomicScanoutRenderDeviceDiscovery,
            >,
        ) {
            let _ = output;
            (
                &mut self.heads[index],
                self.exporters
                    .get_mut(index)
                    .expect("a registered head has an exporter"),
            )
        }

        /// The exporter backing an output's configured primary head, for reads.
        ///
        /// A caller that composes or exports per head must resolve the head
        /// first, or a group's other connectors get nothing.
        fn exporter(
            &self,
            output: OutputId,
        ) -> Option<
            &crate::NativeGbmRenderedScanoutBufferDiscoveryExporter<
                crate::RealAtomicScanoutRenderDeviceDiscovery,
            >,
        > {
            self.exporters.get(self.primary_head_index(output)?)
        }

        /// The exporter backing an output's configured primary head.
        fn exporter_mut(
            &mut self,
            output: OutputId,
        ) -> Result<
            &mut crate::NativeGbmRenderedScanoutBufferDiscoveryExporter<
                crate::RealAtomicScanoutRenderDeviceDiscovery,
            >,
            Box<dyn std::error::Error>,
        > {
            let index = self.primary_head(output)?;
            self.exporters
                .get_mut(index)
                .ok_or_else(|| format!("native output {} has no exporter", output.raw()).into())
        }

        /// The head this logical output is addressed through.
        ///
        /// Every per-head entry point below resolves through this rather than
        /// taking a position, because the caller's position is an index into
        /// *outputs* and the two stop agreeing the moment a mirror group exists.
        fn primary_head(&self, output: OutputId) -> Result<usize, Box<dyn std::error::Error>> {
            self.primary_head_index(output)
                .ok_or_else(|| format!("native output {} has no head", output.raw()).into())
        }

        /// The card driving a logical output.
        pub fn card_for_output(&self, output: OutputId) -> Option<&crate::RealAtomicScanoutCard> {
            self.primary_head_index(output)
                .map(|index| self.card(index))
        }

        /// The primary connector selection of a logical output.
        pub fn selection_for_output(
            &self,
            output: OutputId,
        ) -> Option<crate::LibdrmNativePrimaryPlaneSelection> {
            self.primary_head_index(output)
                .map(|index| self.heads[index].selection)
        }
}
