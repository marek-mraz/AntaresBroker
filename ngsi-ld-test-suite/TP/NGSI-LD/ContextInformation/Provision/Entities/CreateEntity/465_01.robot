*** Settings ***
Documentation       Verify 4.6.5 Supported data types for LanguageMaps.
...
...                 4.6.5: languageMap keys are RFC 5646 language codes or
...                 "@none"; values are JSON strings or arrays of JSON strings.
...                 The encoding {"@none": "urn:ngsi-ld:null"} "shall be used to
...                 represent an NGSI-LD Null during partial update patch and
...                 merge patch" — it is a deletion marker, not a creatable value.
...
...                 Antares extension TP — official LP TPs never send the null
...                 encoding on create nor non-string languageMap values.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
465_01_01 String And Array-Of-Strings Values Round-Trip
    [Documentation]    4.6.5: values are strings or arrays of strings, keys
    ...    language codes or "@none" — accepted and served unchanged; the
    ...    response must NOT contain the NGSI-LD null sentinel.
    [Tags]    e-create    e-retrieve    4_6_5    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "brandName": {"type": "LanguageProperty", "languageMap": {"sk": "škola", "en": ["school", "academy"], "@none": "default"}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${lm}=    Evaluate    $response.json()['brandName']['languageMap']
    ${expected}=    Evaluate    {"sk": "škola", "en": ["school", "academy"], "@none": "default"}
    Should Be Equal    ${lm}    ${expected}
    Should Not Contain    ${response.text}    urn:ngsi-ld:null
    [Teardown]    Delete Entity    ${entity_id}

465_01_02 LanguageMap Null Encoding Is Rejected On Create
    [Documentation]    4.6.5: {"@none": "urn:ngsi-ld:null"} is the patch/merge
    ...    deletion encoding — in a create it is an NGSI-LD Null → 400
    ...    BadRequestData, and the entity must NOT exist afterwards.
    [Tags]    e-create    4_6_5    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "brandName": {"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}

465_01_03 Non-String LanguageMap Value Is Rejected
    [Documentation]    4.6.5: languageMap values "shall be JSON strings or arrays
    ...    of JSON strings" — a numeric value → 400 BadRequestData.
    [Tags]    e-create    4_6_5    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "brandName": {"type": "LanguageProperty", "languageMap": {"en": 5}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
