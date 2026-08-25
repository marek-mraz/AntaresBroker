*** Settings ***
Documentation       Verify the 4.3.6.4 cascade-limiting mechanism on forwards.
...
...                 Clause 4.3.6.4: "It is necessary to include a binding-specific
...                 mechanism to request operations only on the registered endpoint
...                 itself to avoid cascades of an excessive lengths, duplicates or
...                 loops." Clause 5.2.9 localOnly: "If localOnly=true then distributed
...                 operations associated to this Context Source Registration will act
...                 only on data held directly by the registered Context Source itself
...                 (see clause 4.3.6.4)."
...
...                 Antares extension TP — no official TP is tagged 4_3_6_4; nothing
...                 asserts that a forward honouring a localOnly registration carries
...                 the 6.3.18 local parameter.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup LocalOnly Registration And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${stub_entity_filename}                 vehicle-speed-attribute.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
436_04 Forward To A LocalOnly Registration Carries The Local Parameter
    [Documentation]    4.3.6.4/5.2.9: a localOnly registration must only act on data
    ...    held directly by the registered Context Source — the forwarded retrieve
    ...    carries local=true so the source cannot cascade further.
    [Tags]    dist-ops    4_3_6_4    5_7_1    6_3_18    since_v1.9.1
    ${entity_body}=    Load Entity    ${stub_entity_filename}    ${entity_id}
    ${entity_body_json}=    Evaluate    json.dumps($entity_body)    json
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${entity_id}    200    ${entity_body_json}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request
    ${url}=    Get Request Url
    Should Contain    ${url}    local=true


*** Keywords ***
Setup LocalOnly Registration And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ${local_only}=    Create Dictionary    localOnly=${True}
    ${registration_payload}=    Add Object To JSON
    ...    ${registration_payload}
    ...    $
    ...    ${local_only}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
