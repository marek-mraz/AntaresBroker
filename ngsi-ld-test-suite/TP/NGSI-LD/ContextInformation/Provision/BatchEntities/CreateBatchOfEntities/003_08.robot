*** Settings ***
Documentation       Check that an HTTP error response of type BadRequestData is raised if the Content-Type header is "application/ld+json" and the request payload body does not contain a @context term

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
003_08_01 Create A Batch Of One Entity Not Containing A JSON-LD @context With A JSON-LD Content Type
    [Documentation]    Check that an HTTP error response of type BadRequestData is raised if the Content-Type header is "application/ld+json" and the request payload body does not contain a @context term
    [Tags]    be-create    6_3_5
    ${entity_id}=    Generate Random Building Entity Id
    ${entity}=    Load Entity    building-simple-attributes.json    ${entity_id}
    @{entities_to_be_created}=    Create List    ${entity}

    ${response}=    Batch Create Entities    @{entities_to_be_created}    content_type=${CONTENT_TYPE_LD_JSON}

    Check Response Status Code    207    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()['errors'][0]['error']}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element
    ...    ${response.json()['errors'][0]['error']}
