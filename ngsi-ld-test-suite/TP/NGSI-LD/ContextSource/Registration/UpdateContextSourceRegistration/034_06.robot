*** Settings ***
Documentation       Check that one cannot update a context source registration if the Id is not present

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
${reason_405}=                          Method Not Allowed


*** Test Cases ***    FRAGMENT_FILENAME    EXPECTED_STATUS_CODE    REASON
034_06_01 Update A Context Source Registration By Id If The Id Is Not Present
    [Tags]    csr-update    5_9_3
    fragments/context-source-registration-different-type.jsonld    405    ${reason_405}


*** Keywords ***
Update A Context Source
    [Documentation]    Check that one cannot update a context source registration under some conditions
    [Arguments]    ${fragment_filename}    ${expected_status_code}    ${reason}
    ${fragment}=    Load JSON From File    ${EXECDIR}/data/csourceRegistrations/${fragment_filename}
    ${fragment_with_id}=    Update Value To JSON    ${fragment}    $.id    ${EMPTY}
    ${response}=    Update Context Source Registration With Return
    ...    ${EMPTY}
    ...    ${fragment_with_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason}
    Check Response Does Not Contain Body    ${response}

Create Initial Context Source Registration
    ${valid_registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${valid_registration_id}
    ${payload}=    Load JSON From File    ${EXECDIR}/data/csourceRegistrations/${filename}
    ${updated_payload}=    Update Value To JSON    ${payload}    $.id    ${valid_registration_id}
    ${response}=    Create Context Source Registration With Return    ${updated_payload}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Context Source Registration
    Delete Context Source Registration    ${valid_registration_id}
