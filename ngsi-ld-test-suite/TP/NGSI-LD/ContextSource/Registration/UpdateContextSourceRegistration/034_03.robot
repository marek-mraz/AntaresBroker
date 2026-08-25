*** Settings ***
Documentation       Check that one cannot update a context source registration by id if the id is not known to the system

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${filename}=        context-source-registration.jsonld
${reason_404}=      Not Found


*** Test Cases ***
034_03_01 Update A Context Source Registration By Id If The Id Is Not Known To The System
    [Documentation]    Check that one cannot update a context source registration by id if the id is not known to the system
    [Tags]    csr-update    5_9_3
    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${fragment}=    Load JSON From File    ${EXECDIR}/data/csourceRegistrations/${filename}
    ${fragment_with_id}=    Update Value To JSON    ${fragment}    $.id    ${registration_id}
    ${response}=    Update Context Source Registration With Return
    ...    ${registration_id}
    ...    ${fragment_with_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    404    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_404}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
