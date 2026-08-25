*** Settings ***
Documentation       Check that one gets an error when try to delete the core @context

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource
Variables           ${EXECDIR}/resources/variables.py

Test Setup          Delete core context and reload it


*** Variables ***
${reason_400}=      Bad Request
${reason_404}=      Not Found
${reason_422}=      Unprocessable
${type}=            https://uri.etsi.org/ngsi-ld/errors/BadRequestData


*** Test Cases ***
051_09_01 Delete A Core @contexts With Reload Set To True And Check If The Context Has Been Download Again By The Broker
    [Documentation]    Delete a core @contexts with reload set to true and check downloaded core context
    [Tags]    ctx-delete    5_13_5    since_v1.5.1
    # TODO: There is something to change in the spec to faciliate checking if the core context was really updated

    ${response}=    Serve a @context
    ...    contextId=${core_context}
    ...    details=true

    Check Response Status Code    200    ${response.status_code}


*** Keywords ***
Delete core context and reload it
    ${response}=    Serve a @context
    ...    contextId=${core_context}
    ...    details=true
    Check Response Status Code    200    ${response.status_code}

    ${response}=    Delete a @context    ${core_context}    true
    Check Response Status Code    204    ${response.status_code}
