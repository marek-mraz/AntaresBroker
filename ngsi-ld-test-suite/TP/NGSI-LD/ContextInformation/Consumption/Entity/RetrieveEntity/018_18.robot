*** Settings ***
Documentation       Check that a BadRequestDataException is returned if the request is invalid with respect to pick and omit query params

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Created Entity
Test Template       Retrieve Entity With Invalid Pick Or Omit Query Params Usage


*** Variables ***
${filename}=    building-simple-attributes.jsonld


*** Test Cases ***    PICK    OMIT    ATTRS
018_18_01 RetrieveWithSameEntityMemberInPickAndOmit
    [Documentation]    Check that a BadRequestDataException is returned if an entity member is present in pick and omit
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    name    name    ${EMPTY}
018_18_02 RetrieveWithPickAndAttrs
    [Documentation]    Check that a BadRequestDataException is returned if pick and attrs query params are present
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    name    ${EMPTY}    category
018_18_03 RetrieveWithOmitAndAttrs
    [Documentation]    Check that a BadRequestDataException is returned if omit and attrs query params are present
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    ${EMPTY}    name    category
018_18_04 RetrieveWithInvalidCharacter
    [Documentation]    Check that a BadRequestDataException is returned if an invalid character is present
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id;name    ${EMPTY}    ${EMPTY}
018_18_05 RetrieveWithUnclosedBrace
    [Documentation]    Check that a BadRequestDataException is returned if a brace is not closed
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,locatedAt{name    ${EMPTY}    ${EMPTY}
018_18_06 RetrieveWithDoubleBraces
    [Documentation]    Check that a BadRequestDataException is returned if a double brace is present
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,locatedAt{{name}    ${EMPTY}    ${EMPTY}
018_18_07 RetrieveWithConsecutiveSeparators
    [Documentation]    Check that a BadRequestDataException is returned if consecutive separators are present
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,,name    ${EMPTY}    ${EMPTY}
018_18_08 RetrieveWithExpressionStartingWithSpecialCharacter
    [Documentation]    Check that a BadRequestDataException is returned if expression starts with a special character
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,locatedAt{,name}    ${EMPTY}    ${EMPTY}
018_18_09 RetrieveWithExpressionContainingNoAttribute
    [Documentation]    Check that a BadRequestDataException is returned if expression does not contain an attribute
    [Tags]    e-retrieve    5_7_1    4_21    since_v1.8.1
    id,locatedAt{}    ${EMPTY}    ${EMPTY}


*** Keywords ***
Retrieve Entity With Invalid Pick Or Omit Query Params Usage
    [Documentation]    Check that a BadRequestDataException is returned if the request is invalid with respect to pick and omit query params
    [Arguments]    ${pick}    ${omit}    ${attrs}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    ...    attrs=${attrs}
    ...    pick=${pick}
    ...    omit=${omit}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

Create Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}

Delete Created Entity
    Delete Entity    ${entity_id}
