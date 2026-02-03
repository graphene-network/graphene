# TPM2 Tools Configuration for Graphene Node OS
# SPDX-License-Identifier: Apache-2.0

# Include TPM2 tools in the image for attestation
RDEPENDS:${PN}:append = " tpm2-tss"

# Additional tools for attestation
RRECOMMENDS:${PN}:append = " tpm2-abrmd"
