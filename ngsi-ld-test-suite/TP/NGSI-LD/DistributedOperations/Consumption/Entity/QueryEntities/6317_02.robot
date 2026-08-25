*** Settings ***
Documentation       Verify 6.3.17 on an oversized distributed-query response.
...
...                 6.3.17: "invalid non-NGSI-LD payloads shall be rejected
...                 and not incorporated into the overall response" (Table
...                 6.3.17-1, warning 111). A Context Source answering with a
...                 payload above the broker's forwarded-response byte
...                 ceiling (ANTARES_MAX_FED_RESPONSE_BYTES, default 16 MiB)
...                 is treated exactly like one whose payload is invalid:
...                 the part is skipped, the query itself still succeeds.
...
...                 Antares extension TP — the spec sets no response-size
...                 ceiling; this pins the broker's bounds-wall behaviour.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource
Library             Collections

Test Setup          Setup Registration And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld


*** Test Cases ***
6317_02_01 Small Remote Response Is Incorporated
    [Documentation]    Control for 6317_02_02: a normally-sized remote Entity
    ...    Array flows through this exact fixture, so the absence asserted in
    ...    the oversized case is the byte ceiling, not a broken mock.
    [Tags]    dist-ops    6_3_17    since_v1.9.1
    ${serialized_entity}=    Load Entity As Serialized Array    ${entity_payload_filename}    ${entity_id}
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=Vehicle    200    ${serialized_entity}

    &{params}=    Create Dictionary    type=Vehicle
    &{headers}=    Create Dictionary
    ...    Link=<${ngsild_test_suite_context}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any

    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}

6317_02_02 Oversized Remote Response Is Rejected With Warning 111
    [Documentation]    6.3.17: a remote Entity Array above the broker's
    ...    forwarded-response byte ceiling shall NOT be incorporated; the
    ...    part fails like an invalid payload (warning 111) and the query
    ...    itself still answers 200 with the local (empty) result.
    [Tags]    dist-ops    6_3_17    since_v1.9.1
    # one Entity whose Property value alone exceeds the 16 MiB default cap
    ${oversized_body}=    Evaluate
    ...    __import__('json').dumps([{"id": "${entity_id}", "type": "Vehicle", "brandName": {"type": "Property", "value": "x" * (17 * 1024 * 1024)}}])
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=Vehicle    200    ${oversized_body}

    &{params}=    Create Dictionary    type=Vehicle
    &{headers}=    Create Dictionary
    ...    Link=<${ngsild_test_suite_context}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any

    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${entity_id}
    ${warning}=    Get From Dictionary    ${response.headers}    NGSILD-Warning
    Should Contain    ${warning}    111


*** Keywords ***
Setup Registration And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
