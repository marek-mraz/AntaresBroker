*** Settings ***
Documentation       Check that one cannot update a context source registration under some conditions

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Context Source Registration
Test Teardown       Delete Initial Context Source Registration


*** Variables ***
${filename}=                            context-source-registration.jsonld
${registration_payload_file_path}=      context-source-registration-invalid-json.json


*** Test Cases ***
034_04_01 Update A Context Source Registration If The Request Body Is Invalid
    [Documentation]    Check that one cannot update a context source registration if the request body is invalid
    [Tags]    csr-update    5_9_3
    ${response}=    Update Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    ${CONTENT_TYPE_JSON}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}

    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}


*** Keywords ***
Create Initial Context Source Registration
    ${valid_registration_id}=    Generate Random CSR Id
    Set Test Variable    ${valid_registration_id}
    ${registration_id}=    Generate Random CSR Id
    Set Test Variable    ${registration_id}
    ${payload}=    Load JSON From File    ${EXECDIR}/data/csourceRegistrations/${filename}
    ${updated_payload}=    Update Value To JSON    ${payload}    $.id    ${registration_id}
    ${response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Context Source Registration
    Delete Context Source Registration    ${registration_id}
