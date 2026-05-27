export const UNMOTION_CHANNEL_CONFIG = {
  placeholder: "un-motion/frame",
  headingLabelKey: "profiles.editor.unmotion_zenoh_subscriber",
  headingHintKey: "profiles.hints.motion.unmotion_subscriber",
  fieldLabelKey: "profiles.editor.unmotion_zenoh_key",
  fieldHintKey: "profiles.hints.motion.unmotion_key",
  enabledField: "motion.unmotion_zenoh.enabled",
  valueField: "motion.unmotion_zenoh.key",
} as const;

export const VMC_CHANNEL_CONFIG = {
  headingLabelKey: "profiles.editor.vmc_udp_receiver",
  headingHintKey: "profiles.hints.motion.vmc_receiver",
  fieldLabelKey: "profiles.editor.vmc_udp_address",
  fieldHintKey: "profiles.hints.motion.vmc_address",
  enabledField: "motion.vmc_udp.enabled",
  valueField: "motion.vmc_udp.address",
} as const;

export function vmcAddressValue(
  address: string | null,
  port: number | null,
): string {
  return address ?? `0.0.0.0:${port ?? 39539}`;
}
