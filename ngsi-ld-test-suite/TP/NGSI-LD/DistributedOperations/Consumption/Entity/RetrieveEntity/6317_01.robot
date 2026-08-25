*** Settings ***
Documentation       Verify 6.3.17 NGSILD-Warning codes on distributed GET.
...
...                 6.3.17: "NGSILD-Warning HTTP headers shall also be used to
...                 indicate instances of abnormal behaviour for distributed
...                 HTTP GET operations" — 299 for an error response received
...                 from the registration endpoint, 111 for a response whose
...                 payload is invalid; "invalid non-NGSI-LD payloads shall be
...                 rejected and not incorporated into the overall response";
...                 a 404 from the endpoint is NOT abnormal (no warning).
...
...                 Antares extension TP — nothing in the official suite
...                 asserts the NGSILD-Warning header.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource
Library             Collections

Test Setup          Setup Entity And Registration Ids
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
6317_01_01 Error Response From The Endpoint Yields Warning 299
    [Documentation]    6.3.17 Table 6.3.17-1: "An error response (such as
    ...    403 - Forbidden) was received from the registration endpoint" →
    ...    299 Miscellaneous Persistent Warning; the data is not incorporated
    ...    so the retrieve stays 404.
    [Tags]    dist-ops    6_3_17    since_v1.9.1
    Setup Registration And Start Mock
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${entity_id}    403
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    ${warning}=    Get From Dictionary    ${response.headers}    NGSILD-Warning
    Should Contain    ${warning}    299

6317_01_02 Invalid Payload From The Endpoint Yields Warning 111
    [Documentation]    6.3.17: "Although data was received from the
    ...    registration endpoint within the specified timeout period, the
    ...    payload of the response was invalid" → 111 Revalidation Failed,
    ...    and the invalid payload shall NOT be incorporated (404 stays).
    [Tags]    dist-ops    6_3_17    since_v1.9.1
    Setup Registration And Start Mock
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${entity_id}    200    not a json document
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    ${warning}=    Get From Dictionary    ${response.headers}    NGSILD-Warning
    Should Contain    ${warning}    111

6317_01_03 A 404 From The Endpoint Is Not Abnormal
    [Documentation]    6.3.17: "a registration endpoint responding with no
    ...    data and the HTTP status code 404 - Not Found should not be
    ...    considered as abnormal behaviour" — no NGSILD-Warning header.
    [Tags]    dist-ops    6_3_17    since_v1.9.1
    Setup Registration And Start Mock
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${entity_id}    404
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    Dictionary Should Not Contain Key    ${response.headers}    NGSILD-Warning


*** Keywords ***
Setup Entity And Registration Ids
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}

Setup Registration And Start Mock
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
