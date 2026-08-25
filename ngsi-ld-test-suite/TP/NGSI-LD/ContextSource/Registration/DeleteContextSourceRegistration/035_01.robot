*** Settings ***
Documentation       Check that one can delete a context source registration by id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Setup Initial Context Source Registrations


*** Variables ***
${registration_payload_file_path}=      context-source-registration.jsonld


*** Test Cases ***
035_01_01 Delete A Context Source Registration By Id
    [Documentation]    Check that one can delete a context source registration by id
    [Tags]    csr-delete    5_9_4

    ${response}=    Delete Context Source Registration With Return    ${registration_id}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${response1}=    Retrieve Context Source Registration
    ...    context_source_registration_id=${registration_id}
    ...    context=${ngsild_test_suite_context}
    Check SUT Not Containing Resource    ${response1.status_code}


*** Keywords ***
Setup Initial Context Source Registrations
    ${registration_id}=    Generate Random CSR Id
    ${payload}=    Load JSON From File    ${EXECDIR}/data/csourceRegistrations/${registration_payload_file_path}
    ${updated_payload}=    Update Value To JSON    ${payload}    $..id    ${registration_id}
    ${create_response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    201    ${create_response.status_code}
    Set Test Variable    ${registration_id}
