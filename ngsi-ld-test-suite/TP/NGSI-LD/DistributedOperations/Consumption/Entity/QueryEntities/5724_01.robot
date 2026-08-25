*** Settings ***
Documentation       Verify 5.7.2.4: the csf (Context Source Filter, 4.9)
...                 gates which Context Source Registrations are considered
...                 for the distributed query — a matching csf forwards, a
...                 non-matching csf must NOT contact the source (its data
...                 does not appear). Antares extension TP — no official csf
...                 forwarding coverage.

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
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-with-source-type.jsonld


*** Test Cases ***
5724_01_01 Matching Csf Forwards The Query
    [Documentation]    5.7.2.4: the registration carries the Context Source
    ...    Property sourceType="sensor"; csf=sourceType=="sensor" matches →
    ...    the query is forwarded and the remote entity is returned.
    [Tags]    dist-ops    5_7_2_4    since_v1.9.1
    ${serialized_entity}=    Load Entity As Serialized Array    ${entity_payload_filename}    ${entity_id}
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=Vehicle    200    ${serialized_entity}

    &{params}=    Create Dictionary    type=Vehicle    csf=sourceType=="sensor"
    &{headers}=    Create Dictionary
    ...    Link=<${ngsild_test_suite_context}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any

    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.text}    ${entity_id}

5724_01_02 Non-Matching Csf Gates The Source Out
    [Documentation]    5.7.2.4: csf=sourceType=="archive" matches no
    ...    registration — the source must NOT be contacted; the remote
    ...    entity must NOT appear in the response.
    [Tags]    dist-ops    5_7_2_4    since_v1.9.1
    ${serialized_entity}=    Load Entity As Serialized Array    ${entity_payload_filename}    ${entity_id}
    Set Stub Reply    GET    /ngsi-ld/v1/entities?type=Vehicle    200    ${serialized_entity}

    &{params}=    Create Dictionary    type=Vehicle    csf=sourceType=="archive"
    &{headers}=    Create Dictionary
    ...    Link=<${ngsild_test_suite_context}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"
    ${response}=    GET
    ...    url=${url}/entities
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any

    Check Response Status Code    200    ${response.status_code}
    Should Not Contain    ${response.text}    ${entity_id}


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
