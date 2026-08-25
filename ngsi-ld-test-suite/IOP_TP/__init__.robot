*** Settings ***
Documentation       IOP / federation test suite.
...
...                 Self-contained isolation: before and after the whole suite, every broker
...                 (b1_url..b5_url) is reset to a clean slate using ONLY standard NGSI-LD API calls
...                 (see libraries/FederationReset.py). This needs no external scripts and no DB
...                 access, so the ETSI suite stays compact and reusable. Per-test cleanup is handled
...                 by each test's own Test Teardown.

Library             ${EXECDIR}/libraries/FederationReset.py

Suite Setup         Reset All Federation Brokers
Suite Teardown      Reset All Federation Brokers


*** Variables ***
# Overridden at run time with --variable bN_url:http://<broker>/ngsi-ld/v1 . Empty entries are skipped
# by the reset keyword, so this also works for deployments with fewer than five brokers.
${b1_url}           ${EMPTY}
${b2_url}           ${EMPTY}
${b3_url}           ${EMPTY}
${b4_url}           ${EMPTY}
${b5_url}           ${EMPTY}


*** Keywords ***
Reset All Federation Brokers
    Reset Federation Brokers    ${b1_url}    ${b2_url}    ${b3_url}    ${b4_url}    ${b5_url}
