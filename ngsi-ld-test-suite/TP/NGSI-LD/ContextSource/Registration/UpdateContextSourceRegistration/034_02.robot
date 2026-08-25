*** Settings ***
Documentation       Check that one cannot update a context source registration under some conditions

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Context Source Registration
Test Teardown       Delete Initial Context Source Registration
Test Template       Update A Context Source


*** Variables ***
${filename}=                            context-source-registration.jsonld
${registration_payload_file_path}=      context-source-registration-invalid.jsonld
${reason_400}=                          Bad Request


*** Test Cases ***    REGISTRATION_ID    FRAGMENT_FILENAME    EXPECTED_STATUS_CODE    PROBLEM_TYPE
034_02_01 Update A Context Source Registration By Id If The Id Is Not A Valid URI
    [Tags]    csr-update    5_9_3
    invalidURI    fragments/context-source-registration-different-type.jsonld    400    ${ERROR_TYPE_BAD_REQUEST_DATA}
034_02_02 Update A Context Source Registration If The Request Body Is Not Of The Same Data Type
    [Tags]    csr-update    5_9_3
    ${valid_registration_id}    fragments/context-source-registration-different-type.jsonld    400    ${ERROR_TYPE_BAD_REQUEST_DATA}
034_02_03 Update A Context Source Registration If One Attempts To Remove A Mandatory Property
    [Tags]    csr-update    5_9_3
    ${valid_registration_id}    context-source-registration-invalid.jsonld    400    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Update A Context Source
    [Documentation]    Check that one cannot update a context source registration under some conditions
    [Arguments]    ${registration_id}    ${fragment_filename}    ${expected_status_code}    ${problem_type}
    ${fragment}=    Load JSON From File    ${EXECDIR}/data/csourceRegistrations/${fragment_filename}
    ${fragment_with_id}=    Update Value To JSON    ${fragment}    $.id    ${registration_id}
    ${response}=    Update Context Source Registration With Return
    ...    ${registration_id}
    ...    ${fragment_with_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_400}

    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${problem_type}

Create Initial Context Source Registration
    ${valid_registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${valid_registration_id}
    ${payload}=    Load JSON From File    ${EXECDIR}/data/csourceRegistrations/${filename}
    ${updated_payload}=    Update Value To JSON    ${payload}    $.id    ${valid_registration_id}
    ${response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Context Source Registration
    Delete Context Source Registration    ${valid_registration_id}
