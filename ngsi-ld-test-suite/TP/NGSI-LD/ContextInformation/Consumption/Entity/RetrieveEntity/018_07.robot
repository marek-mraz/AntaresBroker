*** Settings ***
Documentation       Check that one can retrieve an entity using Language Filter

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Initial Entity
Suite Teardown      Delete Created Entity
Test Template       Retrieve Entity With Language Filter


*** Variables ***
${filename}=    building-language-property.jsonld


*** Test Cases ***    LANGUAGE_FILTER    OPTIONS    EXPECTATION_FILENAME
018_07_01 Retrieve An Entity Using A Specific Natural Language
    [Tags]    e-retrieve    5_7_1    4_15    since_v1.4.1
    fr    ${EMPTY}    building-language-property-fr-filter.jsonld
018_07_02 Retrieve An Entity Using Multiple Natural Languages With No Ranked Preference
    [Tags]    e-retrieve    5_7_1    4_15    since_v1.4.1
    fr-CH,fr    ${EMPTY}    building-language-property-fr-filter.jsonld
018_07_03 Retrieve An Entity With Any Supported Language
    [Tags]    e-retrieve    5_7_1    4_15    since_v1.4.1
    *    ${EMPTY}    building-language-property-any-language-filter.jsonld
018_07_04 Retrieve An Entity Using Multiple Natural Languages With Ranked Preferences
    [Tags]    e-retrieve    5_7_1    4_15    since_v1.4.1
    fr-CH,fr;q=0.9,en;q=0.8,*;q=0.5    ${EMPTY}    building-language-property-fr-filter.jsonld
018_07_05 Retrieve An Entity Using A Specific Natural Language With Simplified Representation
    [Tags]    e-retrieve    5_7_1    4_15    since_v1.4.1
    fr    keyValues    building-language-property-fr-filter-simplified.jsonld
018_07_06 Retrieve An Entity With Any Supported Language With Simplified Representation
    [Tags]    e-retrieve    5_7_1    4_15    since_v1.4.1
    *    keyValues    building-language-property-any-language-filter-simplified.jsonld


*** Keywords ***
Retrieve Entity With Language Filter
    [Documentation]    Check that one can retrieve an entity using Language Filter
    [Arguments]    ${language_filter}    ${options}    ${expectation_filename}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    ...    context=${ngsild_test_suite_context}
    ...    options=${options}
    ...    lang=${language_filter}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Entity element
    ...    ${expectation_filename}
    ...    ${entity_id}
    ...    ${response.json()}

Create Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Created Entity
    Delete Entity    ${entity_id}
