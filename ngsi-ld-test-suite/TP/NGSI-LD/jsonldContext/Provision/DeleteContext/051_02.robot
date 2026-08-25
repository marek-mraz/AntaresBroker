*** Settings ***
Documentation       Check that one gets an error when trying to delete an unknown @context identifier

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Variables ***
${reason_404}=      Not Found


*** Test Cases ***
051_02_01 Delete A @context With Unknown @context Identifier
    [Documentation]    Check that an error message is obtained in the response when one tries to delete a @context with unknonwn id
    [Tags]    ctx-serve    5_13_5    since_v1.5.1

    ${random_url}=    Generate Random String    16    [NUMBERS]
    ${response}=    Delete a @context    ${random_url}

    Check Response Status Code    404    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_404}

    Check Response Body Containing ProblemDetails Element    ${response.json()}    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
