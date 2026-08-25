*** Settings ***
Documentation       4.22 transient entity end-to-end lifecycle (Antares extension TP).
...                 Create an entity expiring in 3 s, see it served, wait 5 s, see it
...                 gone — then re-create the SAME id and expect 201: that only works
...                 if the garbage collector really reaped the row (a read-boundary
...                 filter alone would still answer 409 AlreadyExists). Requires the
...                 broker to sweep faster than the wait — the ETSI stack runs
...                 ANTARES_SWEEP_SECS=2 (compose-files/docker-compose-etsi.yml).

Library             DateTime
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Teardown      Delete Entity    ${entity_id}


*** Test Cases ***
422_02_01 Transient Entity Lifecycle Create Serve Expire Reap
    [Tags]    transient    4_22
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${now}=    Get Current Date    time_zone=UTC
    ${expiry}=    Add Time To Date    ${now}    3s    result_format=%Y-%m-%dT%H:%M:%SZ
    ${entity}=    Evaluate
    ...    {"id": $entity_id, "type": "Building", "name": {"type": "Property", "value": "short-lived"}, "expiresAt": $expiry, "@context": $ngsild_test_suite_context}
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
    # Served while alive
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Sleep    5s
    # Invalid past expiresAt
    ${response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    # Reaped, not merely hidden: the id is free again (GC proof). 4.22 says
    # deletion "will always lag the expiresAt timestamp to a certain extent" —
    # here up to one ANTARES_SWEEP_SECS tick — so poll briefly instead of
    # asserting one racy instant.
    Wait Until Keyword Succeeds    6s    1s    Recreate Succeeds    ${entity}


*** Keywords ***
Recreate Succeeds
    [Arguments]    ${entity}
    ${response}=    Create Entity From JSON-LD Content    ${entity}
    Check Response Status Code    201    ${response.status_code}
