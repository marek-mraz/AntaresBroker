*** Settings ***
Documentation       Check that one can create a context source registration without specifying an ID

Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Teardown      Delete Created Context Source Registrations


*** Variables ***
${registration_payload_file_path}=      csourceRegistrations/context-source-registration-no-id.jsonld


*** Test Cases ***
033_11 Create Context Source Registration Without Specifying An ID
    [Documentation]    Check that one can create a context source registration without specifying an ID
    [Tags]    csr-create    5_9_2
    ${payload}=    Load JSON From File    ${EXECDIR}/data/${registration_payload_file_path}
    ${response}=    Create Context Source Registration With Return    ${payload}
    Check Response Status Code    201    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${registration_id}=    Check Response Headers ID Not Empty    ${response.headers}
    Set Suite Variable    ${registration_id}
    ${id_dict}=    Create Dictionary    id=${registration_id}
    ${registration_payload}=    Add Object To Json    ${payload}    $    ${id_dict}
    ${response1}=    Retrieve Context Source Registration
    ...    context_source_registration_id=${registration_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Response Body Containing Context Source Registration element
    ...    ${registration_payload_file_path}
    ...    ${registration_id}
    ...    ${response1.json()}


*** Keywords ***
Delete Created Context Source Registrations
    Delete Context Source Registration    ${registration_id}
