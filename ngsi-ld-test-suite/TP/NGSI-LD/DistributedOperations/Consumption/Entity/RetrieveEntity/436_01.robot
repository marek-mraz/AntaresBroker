*** Settings ***
Documentation       Verify the 4.3.6.1 distributed-request contract on retrieve.
...
...                 Clause 4.3.6.1: "It is the responsibility of the Context Broker to
...                 respect the registration parameters when issuing distributed
...                 requests. [...] Ultimately, all constraints specified in the
...                 registration shall be respected." And: registered Context Sources
...                 "may indicate that they are only willing to respond to a limited
...                 subset of API operations. Context Brokers shall respect this, to
...                 avoid unnecessarily sending distributed operation requests which
...                 are always guaranteed to fail."
...
...                 Antares extension TP — the official D0xx set proves the positive
...                 side (an allowed operation is forwarded); nothing covers the
...                 negative gate (an excluded operation must NOT be forwarded) or the
...                 narrowing of the forwarded request to the registered attributes.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Entity And Registration Ids
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${stub_entity_filename}                 vehicle-speed-attribute.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
436_01_01 Read Request Is Not Forwarded To A Registration Excluding Read Operations
    [Documentation]    4.3.6.1: Context Brokers shall respect a registration's limited
    ...    operations subset. A matching inclusive registration whose operations list
    ...    carries only updateOps must not receive a forwarded retrieve; with no local
    ...    entity the broker answers 404 ResourceNotFound instead.
    [Tags]    dist-ops    4_3_6_1    5_7_1    since_v1.9.1
    ${operations}=    Create List    updateOps
    Setup Registration With Operations And Start Mock    ${operations}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    Wait For No Request

436_01_02 Forwarded Retrieve Is Narrowed To The Registered PropertyNames
    [Documentation]    4.3.6.1: all constraints specified in the registration shall be
    ...    respected when issuing distributed requests. The registration covers only the
    ...    speed Property, so the forwarded retrieve carries attrs=speed rather than the
    ...    consumer's unrestricted request.
    [Tags]    dist-ops    4_3_6_1    5_7_1    since_v1.9.1
    ${operations}=    Create List    retrieveEntity
    Setup Registration With Operations And Start Mock    ${operations}
    ${entity_body}=    Load Entity    ${stub_entity_filename}    ${entity_id}
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${entity_id}    200    ${entity_body}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request
    ${url}=    Get Request Url
    Should Contain    ${url}    attrs=speed
    Should Not Contain    ${url}    brandName


*** Keywords ***
Setup Entity And Registration Ids
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}

Setup Registration With Operations And Start Mock
    [Arguments]    ${operations}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ...    operations=${operations}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
