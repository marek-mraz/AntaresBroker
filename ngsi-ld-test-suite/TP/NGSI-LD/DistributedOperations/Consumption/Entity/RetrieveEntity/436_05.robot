*** Settings ***
Documentation       Verify the 4.3.6.5 contextSourceInfo conveyance rules on forwards.
...
...                 Clause 4.3.6.5: contextSourceInfo "contains whatever extra
...                 information the Context Broker shall convey when contacting the
...                 Context Source [...] using headers in the case of HTTP." The
...                 special value "urn:ngsi-ld:request" copies the value "from the
...                 request that triggered the given request, if present." Tenant
...                 information "shall not be part of contextSourceInfo" and
...                 binding-specific interaction information "shall be ignored."
...
...                 Antares extension TP — no official TP touches contextSourceInfo.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${stub_entity_filename}                 vehicle-speed-attribute.json
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld


*** Test Cases ***
436_05_01 ContextSourceInfo Pairs Are Conveyed As Headers
    [Documentation]    4.3.6.5: the Context Broker shall convey the registered
    ...    key-value pairs when contacting the Context Source — as headers in HTTP.
    [Tags]    dist-ops    4_3_6_5    5_7_1    since_v1.9.1
    ${csi}=    Evaluate
    ...    [{"key": "Authorization", "value": "Bearer test-token"}, {"key": "X-Custom-Info", "value": "fixed"}]
    Setup Registration With ContextSourceInfo And Start Mock    ${csi}
    Stub Entity And Retrieve    ${entity_id}
    Wait For Request
    ${headers}=    Get Request Headers
    ${auth}=    Evaluate    $headers.get('Authorization')
    Should Be Equal    ${auth}    Bearer test-token
    ${custom}=    Evaluate    $headers.get('X-Custom-Info')
    Should Be Equal    ${custom}    fixed

436_05_02 The urn:ngsi-ld:request Value Copies The Triggering Header
    [Documentation]    4.3.6.5: "the special value urn:ngsi-ld:request can be used to
    ...    indicate that the respective value is to be taken from the request that
    ...    triggered the given request, if present" — present it is copied, absent
    ...    nothing is conveyed.
    [Tags]    dist-ops    4_3_6_5    5_7_1    since_v1.9.1
    ${csi}=    Evaluate    [{"key": "user", "value": "urn:ngsi-ld:request"}]
    Setup Registration With ContextSourceInfo And Start Mock    ${csi}
    ${entity_body}=    Load Entity    ${stub_entity_filename}    ${entity_id}
    ${entity_body_json}=    Evaluate    json.dumps($entity_body)    json
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${entity_id}    200    ${entity_body_json}
    # triggering request carries user: abcd → conveyed
    ${response}=    Retrieve Entity With Extra Header    ${entity_id}    user    abcd
    Check Response Status Code    200    ${response.status_code}
    Wait For Request
    ${headers}=    Get Request Headers
    ${user}=    Evaluate    $headers.get('user')
    Should Be Equal    ${user}    abcd
    # triggering request without user → nothing conveyed
    ${response}=    Retrieve Entity With Extra Header    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Wait For Request
    ${headers}=    Get Request Headers
    ${user}=    Evaluate    $headers.get('user')
    Should Be Equal    ${user}    ${None}

436_05_03 Tenant And Connection Headers Cannot Be Overridden
    [Documentation]    4.3.6.5: Tenant information "shall not be part of
    ...    contextSourceInfo" and binding-specific interaction information (Via,
    ...    Content-Length, ...) "shall be ignored" — such pairs never reach the wire.
    [Tags]    dist-ops    4_3_6_5    5_7_1    since_v1.9.1
    ${csi}=    Evaluate
    ...    [{"key": "NGSILD-Tenant", "value": "hacker"}, {"key": "Via", "value": "1.1 evil"}]
    Setup Registration With ContextSourceInfo And Start Mock    ${csi}
    Stub Entity And Retrieve    ${entity_id}
    Wait For Request
    ${headers}=    Get Request Headers
    ${tenant}=    Evaluate    $headers.get('NGSILD-Tenant')
    Should Be Equal    ${tenant}    ${None}
    ${via}=    Evaluate    str($headers.get('Via'))
    Should Not Contain    ${via}    evil


*** Keywords ***
Setup Registration With ContextSourceInfo And Start Mock
    [Arguments]    ${csi}
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ${csi_dict}=    Create Dictionary    contextSourceInfo=${csi}
    ${registration_payload}=    Add Object To JSON
    ...    ${registration_payload}
    ...    $
    ...    ${csi_dict}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
    Start Context Source Mock Server

Stub Entity And Retrieve
    [Arguments]    ${id}
    ${entity_body}=    Load Entity    ${stub_entity_filename}    ${id}
    ${entity_body_json}=    Evaluate    json.dumps($entity_body)    json
    Set Stub Reply    GET    /ngsi-ld/v1/entities/${id}    200    ${entity_body_json}
    ${response}=    Retrieve Entity    ${id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}

Retrieve Entity With Extra Header
    [Arguments]    ${id}    ${key}=${EMPTY}    ${value}=${EMPTY}
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    &{headers}=    Create Dictionary    Link=${context_link}
    IF    '${key}' != ''
        Set To Dictionary    ${headers}    ${key}=${value}
    END
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${id}
    ...    headers=${headers}
    ...    expected_status=any
    RETURN    ${response}

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
