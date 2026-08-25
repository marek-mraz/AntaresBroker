*** Settings ***
Documentation       Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Created Context Source Registrations


*** Variables ***
${registration_payload_file_path}=      csourceRegistrations/context-source-registration.json


*** Test Cases ***
033_04_01 Create One Context Source Registration Using A Provided Link Header With JSON Content Type With Context
    [Documentation]    Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
    [Tags]    csr-create    6_3_5
    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${payload}=    Load JSON From File    ${EXECDIR}/data/${registration_payload_file_path}
    ${updated_payload}=    Update Value To JSON    ${payload}    $..id    ${registration_id}
    ${response}=    Create Context Source Registration With Return
    ...    ${updated_payload}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Context Source Registration
    ...    context_source_registration_id=${registration_id}
    ...    context=${ngsild_test_suite_context}
    Check JSON Value In Response Body
    ...    ['information'][0]['entities'][0]['type']
    ...    Building
    ...    ${response1.json()}

033_04_02 Create One Context Source Registration Using A Provided Link Header With JSON Content Type Without Context
    [Documentation]    Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
    [Tags]    csr-create    6_3_5
    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${payload}=    Load JSON From File    ${EXECDIR}/data/${registration_payload_file_path}
    ${updated_payload}=    Update Value To JSON    ${payload}    $..id    ${registration_id}
    ${response}=    Create Context Source Registration With Return
    ...    ${updated_payload}
    ...    ${CONTENT_TYPE_JSON}
    ...    ${ngsild_test_suite_context}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Context Source Registration
    ...    context_source_registration_id=${registration_id}
    Check JSON Value In Response Body
    ...    ['information'][0]['entities'][0]['type']
    ...    https://ngsi-ld-test-suite/context#Building
    ...    ${response1.json()}


*** Keywords ***
Delete Created Context Source Registrations
    Delete Context Source Registration    ${registration_id}
